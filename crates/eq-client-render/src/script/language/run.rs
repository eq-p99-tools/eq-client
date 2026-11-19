//! A cooperative interpreter. An action is handed off to the ordinary script driver.

use std::time::Duration;

use eq_client_core::world::ClientWorld;

use super::{
    Code, Value,
    compile::{MAX_ITERATIONS, Op, Variable},
    observe::Field,
};
use crate::script::{Instruction, QueuedStep, RunStatus, parse::MAX_STEPS};

const MAX_EXECUTED: usize = 5_000;
/// Pace repeated actions independently of frame rate. Linear scripts keep their normal cadence.
const ACTION_INTERVAL: Duration = Duration::from_millis(100);

pub(in crate::script) enum Tick {
    Action(QueuedStep),
    Logic,
}

pub(in crate::script) struct Run {
    code: Code,
    actions: Vec<Instruction>,
    values: Vec<Value>,
    loops: Vec<Iteration>,
    pc: usize,
    next_action: Duration,
    yielded_actions: usize,
}

struct Iteration {
    header: usize,
    number: u16,
    limit: u16,
}

impl Run {
    pub(in crate::script) fn new(code: Code, actions: Vec<Instruction>) -> Self {
        let values = vec![Value::Unknown; code.variables.len()];
        Self {
            code,
            actions,
            values,
            loops: Vec::new(),
            pc: 0,
            next_action: Duration::ZERO,
            yielded_actions: 0,
        }
    }

    pub(in crate::script) fn done(&self) -> bool {
        self.pc == self.code.ops.len()
    }

    pub(in crate::script) fn paced(&self, now: Duration) -> bool {
        now < self.next_action
            && self
                .code
                .ops
                .get(self.pc)
                .is_some_and(|located| matches!(located.op, Op::Action(_)))
    }

    pub(in crate::script) fn tick(
        &mut self,
        world: &ClientWorld,
        progress: &mut RunStatus,
        now: Duration,
    ) -> Result<Tick, String> {
        self.execute(&|field| field.read(world), progress, now)
    }

    #[allow(
        clippy::too_many_lines,
        reason = "one match over every operation keeps control flow in one place"
    )]
    fn execute(
        &mut self,
        read: &impl Fn(Field) -> Value,
        progress: &mut RunStatus,
        now: Duration,
    ) -> Result<Tick, String> {
        let Some(located) = self.code.ops.get(self.pc) else {
            return Ok(Tick::Logic);
        };
        progress.line = Some(located.line);
        let flow = progress
            .flow
            .as_mut()
            .expect("structured run has flow status");
        if flow.statements == MAX_EXECUTED {
            return Err(format!(
                "maximum of {MAX_EXECUTED} executed statements reached"
            ));
        }
        flow.statements += 1;

        let header = self.pc;
        self.pc += 1;
        let tick = match &located.op {
            Op::Action(index) => {
                if self.yielded_actions == MAX_STEPS {
                    return Err(format!("maximum of {MAX_STEPS} executed actions reached"));
                }
                self.yielded_actions += 1;
                let action = &self.actions[*index];
                self.next_action = now + ACTION_INTERVAL;
                Tick::Action(QueuedStep {
                    step: action.step.clone(),
                    line: Some(action.line),
                })
            }
            Op::Store(index, expression) => {
                let value = expression.evaluate(&self.values, read)?;
                let variable = &self.code.variables[*index];
                let name = variable.name.clone();
                Self::store(
                    &self.code.variables,
                    &mut self.values,
                    &mut flow.variables,
                    *index,
                    value,
                );
                progress.event(format!("Set {name}"));
                Tick::Logic
            }
            Op::If {
                condition,
                otherwise,
            } => {
                let take = boolean(&condition.evaluate(&self.values, read)?)?;
                flow.branch = Some((located.line, take));
                if !take {
                    self.pc = *otherwise;
                }
                progress.event(format!("If condition is {take}"));
                Tick::Logic
            }
            Op::Repeat {
                count,
                exit,
                variable,
            } => {
                let (exit, variable) = (*exit, *variable);
                let limit = if let Some(iteration) =
                    self.loops.last().filter(|entry| entry.header == header)
                {
                    iteration.limit
                } else {
                    repeat_count(&count.evaluate(&self.values, read)?)?
                };
                self.iteration(progress, header, exit, limit, true);
                if let Some(variable) = variable
                    && let Some(entry) = self.loops.last().filter(|entry| entry.header == header)
                {
                    let value = Value::Int(i64::from(entry.number) - 1);
                    let flow = progress
                        .flow
                        .as_mut()
                        .expect("structured run has flow status");
                    Self::store(
                        &self.code.variables,
                        &mut self.values,
                        &mut flow.variables,
                        variable,
                        value,
                    );
                }
                Tick::Logic
            }
            Op::Break(loop_header) => {
                if self
                    .loops
                    .last()
                    .is_some_and(|entry| entry.header == *loop_header)
                {
                    self.loops.pop();
                }
                self.pc = match &self.code.ops[*loop_header].op {
                    Op::Repeat { exit, .. } | Op::While { exit, .. } => *exit,
                    _ => unreachable!("break targets a loop header"),
                };
                self.show_loops(progress);
                progress.event("Break");
                Tick::Logic
            }
            Op::Continue(loop_header) => {
                self.pc = *loop_header;
                progress.event("Continue");
                Tick::Logic
            }
            Op::While {
                condition,
                limit,
                exit,
            } => {
                let exhausted = self
                    .loops
                    .last()
                    .is_some_and(|entry| entry.header == header && entry.number == entry.limit);
                let take = exhausted || boolean(&condition.evaluate(&self.values, read)?)?;
                self.iteration(progress, header, *exit, *limit, take);
                Tick::Logic
            }
            Op::Jump(_) => unreachable!("internal jumps are normalized at the statement boundary"),
        };
        // Forward skips and the loop backedge do not use a step or run any code.
        // Every backedge lands on a visible loop header, so chains are bounded.
        while let Some(Op::Jump(destination)) =
            self.code.ops.get(self.pc).map(|located| &located.op)
        {
            self.pc = *destination;
        }
        Ok(tick)
    }

    fn iteration(
        &mut self,
        progress: &mut RunStatus,
        header: usize,
        exit: usize,
        limit: u16,
        take: bool,
    ) {
        let mut entering = self.loops.last_mut().filter(|entry| entry.header == header);
        let finished = entering
            .as_ref()
            .is_some_and(|entry| entry.number == entry.limit);
        let event = if !take || limit == 0 || finished {
            if entering.is_some() {
                self.loops.pop();
            }
            self.pc = exit;
            match (take, limit, &self.code.ops[header].op) {
                (false, _, _) => "While condition is false".into(),
                (_, 0, _) => "Range count is zero".into(),
                (_, _, Op::While { .. }) => "While iteration limit reached".into(),
                _ => "For loop finished".into(),
            }
        } else {
            let number = if let Some(entry) = entering.take() {
                entry.number += 1;
                entry.number
            } else {
                self.loops.push(Iteration {
                    header,
                    number: 1,
                    limit,
                });
                1
            };
            format!("Iteration {number}/{limit}")
        };
        self.show_loops(progress);
        progress.event(event);
    }

    fn show_loops(&self, progress: &mut RunStatus) {
        progress
            .flow
            .as_mut()
            .expect("structured run has flow status")
            .loops = self
            .loops
            .iter()
            .map(|entry| (self.code.ops[entry.header].line, entry.number, entry.limit))
            .collect();
    }

    /// Set a variable and its line in the state panel. Summaries are kept by
    /// variable; one not yet set, such as a loop's that never ran, is empty.
    fn store(
        variables: &[Variable],
        values: &mut [Value],
        summaries: &mut Vec<String>,
        index: usize,
        value: Value,
    ) {
        let variable = &variables[index];
        let summary = format!("{}: {} = {value}", variable.name, variable.kind);
        values[index] = value;
        if summaries.len() <= index {
            summaries.resize(index + 1, String::new());
        }
        summaries[index] = summary;
    }
}

fn boolean(value: &Value) -> Result<bool, String> {
    match value {
        Value::Bool(value) => Ok(*value),
        Value::Unknown | Value::Stale => Err(format!(
            "condition is {value}; guard unavailable values with known() or stale()"
        )),
        _ => unreachable!("condition type was checked during compilation"),
    }
}

fn repeat_count(value: &Value) -> Result<u16, String> {
    match value {
        Value::Int(value) if (0..=MAX_ITERATIONS).contains(value) => {
            Ok(u16::try_from(*value).expect("iteration count is bounded"))
        }
        Value::Int(_) => Err(format!("range count must be from 0 to {MAX_ITERATIONS}")),
        Value::Unknown | Value::Stale => Err(format!("range count is {value}")),
        _ => unreachable!("range count type was checked during compilation"),
    }
}

#[cfg(test)]
mod tests {
    use std::{fmt::Write as _, path::Path, time::Duration};

    use super::*;
    use crate::script::{Program, Step, execution::FlowStatus};

    /// Runs a program offline to its end: its reports, in order.
    fn reports(source: &str) -> Result<Vec<String>, String> {
        let program = Program::parse(source, Path::new(".")).unwrap();
        let (_, actions, code) = program.into_parts();
        let mut run = Run::new(code.expect("a structured program"), actions);
        let mut status = RunStatus {
            flow: Some(FlowStatus::default()),
            ..RunStatus::default()
        };
        let mut reports = Vec::new();
        let mut now = Duration::ZERO;
        while !run.done() {
            now += Duration::from_secs(1);
            if let Tick::Action(queued) = run.execute(&|_| Value::Unknown, &mut status, now)?
                && let Step::Report(text) = queued.step
            {
                reports.push(text);
            }
        }
        Ok(reports)
    }

    #[test]
    fn an_elif_chain_takes_exactly_one_branch() {
        for (n, taken) in [(1, "one"), (2, "two"), (3, "three"), (5, "other")] {
            let source = format!(
                "n: int = {n}\nif n == 1:\n    report(\"one\")\nelif n == 2:\n    report(\"two\")\nelif n == 3:\n    report(\"three\")\nelse:\n    report(\"other\")\nreport(\"after\")\n"
            );
            assert_eq!(reports(&source).unwrap(), [taken, "after"], "{n}");
        }
        // An elif chain is one nesting level, so a chain can be long.
        let mut chain = "n: int = 20\nif n == 0:\n    report(\"0\")\n".to_owned();
        for k in 1..=20 {
            write!(chain, "elif n == {k}:\n    report(\"{k}\")\n").unwrap();
        }
        assert_eq!(reports(&chain).unwrap(), ["20"]);
    }

    #[test]
    fn for_loops_count_from_zero_and_break_and_continue_their_own_loop() {
        let source = "total: int = 0\nfor i in range(6):\n    if i == 1:\n        continue\n    if i == 4:\n        break\n    total += i\n    report(\"tick\")\nif total == 5:\n    report(\"five\")\n";
        assert_eq!(reports(source).unwrap(), ["tick", "tick", "tick", "five"]);
        let nested = "for i in range(2):\n    for j in range(3):\n        if j == 1:\n            break\n        report(\"inner\")\n    report(\"outer\")\n";
        assert_eq!(
            reports(nested).unwrap(),
            ["inner", "outer", "inner", "outer"]
        );
        let sibling = "for i in range(2):\n    pass\nfor i in range(1):\n    if i == 0:\n        report(\"again\")\n";
        assert_eq!(reports(sibling).unwrap(), ["again"]);
        assert_eq!(
            reports("for _ in range(0):\n    report(\"never\")\n").unwrap(),
            [""; 0]
        );
        assert!(
            reports("n: int = 101\nfor _ in range(n):\n    pass\n")
                .unwrap_err()
                .contains("from 0 to 100")
        );
    }

    #[test]
    fn a_while_cap_counts_iterations_a_continue_ends() {
        let source = "n: int = 0\nwhile True, limit=5:\n    n += 1\n    if n < 3:\n        continue\n    report(\"late\")\n";
        assert_eq!(reports(source).unwrap(), ["late", "late", "late"]);
        let broken = "n: int = 0\nwhile n < 50, limit=100:\n    n += 1\n    if n == 2:\n        break\n    report(\"once\")\nif n == 2:\n    report(\"left at two\")\n";
        assert_eq!(reports(broken).unwrap(), ["once", "left at two"]);
    }

    #[test]
    fn loop_counters_are_read_only_and_scoped_to_their_loop() {
        let base = Path::new(".");
        for (source, message) in [
            ("for i in range(2):\n    i = 1\n", "loop variable"),
            ("for i in range(2):\n    i += 1\n", "loop variable"),
            (
                "for i in range(2):\n    for i in range(2):\n        pass\n",
                "already declared",
            ),
            (
                "i: int = 0\nfor i in range(2):\n    pass\n",
                "already declared",
            ),
            (
                "for i in range(2):\n    pass\nj: int = i\n",
                "unknown variable",
            ),
            ("for i in range(True):\n    pass\n", "expected int"),
            ("for i in [1, 2]:\n    pass\n", "range(count)"),
            ("for wait in range(2):\n    pass\n", "reserved"),
            ("t: text = \"a\"\nt += \"b\"\n", "int"),
            ("break\n", "outside a loop"),
            ("if True:\n    continue\n", "outside a loop"),
        ] {
            let errors = Program::diagnostics(source, base);
            assert!(
                errors
                    .first()
                    .is_some_and(|error| error.message.contains(message)),
                "{source}: {errors:?}"
            );
        }
    }
}
