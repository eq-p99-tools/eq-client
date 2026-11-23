//! A cooperative interpreter. An action is handed off to the ordinary script driver.

use std::time::Duration;

use super::{
    Code, Value,
    compile::{MAX_ITERATIONS, Op, Variable},
    observe::{Field, Sight},
};
use crate::script::{Instruction, QueuedStep, RunStatus, parse::MAX_STEPS};

const MAX_EXECUTED: usize = 5_000;
/// Nested calls, recursion included.
const MAX_DEPTH: usize = 8;
/// Pace repeated actions independently of frame rate. Linear scripts keep their normal cadence.
const ACTION_INTERVAL: Duration = Duration::from_millis(100);

pub(in crate::script) enum Tick {
    Action(QueuedStep),
    Logic,
}

pub(in crate::script) struct Run {
    code: Code,
    actions: Vec<Instruction>,
    /// Each slot's value; `None` until its declaration or assignment runs.
    values: Vec<Option<Value>>,
    /// Each slot's line for the state panel, empty until set.
    summaries: Vec<String>,
    /// The loops open in the running function, or at the top level.
    loops: Vec<Iteration>,
    /// One entry per call in progress, innermost last.
    frames: Vec<Frame>,
    pc: usize,
    next_action: Duration,
    yielded_actions: usize,
    /// The `else` line just entered by a false condition, until the driver
    /// checks it for a breakpoint.
    edge: Option<usize>,
}

/// A call in progress: where it returns to, and what it set aside.
struct Frame {
    function: usize,
    return_pc: usize,
    /// The call's line.
    line: usize,
    /// The function's slots as the caller had them, restored on return,
    /// so a recursive call has its own.
    saved: Vec<(Option<Value>, String)>,
    /// The caller's open loops.
    loops: Vec<Iteration>,
}

struct Iteration {
    header: usize,
    number: u16,
    limit: u16,
}

impl Run {
    pub(in crate::script) fn new(code: Code, actions: Vec<Instruction>) -> Self {
        let values = vec![None; code.variables.len()];
        let summaries = vec![String::new(); code.variables.len()];
        let mut run = Self {
            code,
            actions,
            values,
            summaries,
            loops: Vec::new(),
            frames: Vec::new(),
            pc: 0,
            next_action: Duration::ZERO,
            yielded_actions: 0,
            edge: None,
        };
        run.skip_jumps();
        run
    }

    /// The line of the statement that runs next, if any. A function's
    /// implicit return at the end of its body is no statement.
    pub(in crate::script) fn next_line(&self) -> Option<usize> {
        if self.done() {
            return None;
        }
        self.code
            .ops
            .get(self.pc)
            .filter(|located| !matches!(located.op, Op::Return { explicit: false }))
            .map(|located| located.line)
    }

    /// Calls in progress.
    pub(in crate::script) fn depth(&self) -> usize {
        self.frames.len()
    }

    /// Whether the next op calls a function.
    pub(in crate::script) fn calls_next(&self) -> bool {
        self.code
            .ops
            .get(self.pc)
            .is_some_and(|located| matches!(located.op, Op::Call { .. }))
    }

    /// The `else` entered since the last check, once.
    pub(in crate::script) const fn take_edge(&mut self) -> Option<usize> {
        self.edge.take()
    }

    /// The op that runs next; the end of the program when done.
    pub(in crate::script) const fn position(&self) -> usize {
        self.pc
    }

    /// The ops of the innermost block around the next statement, its header
    /// included.
    pub(in crate::script) fn innermost_block(&self) -> Option<std::ops::Range<usize>> {
        self.code
            .blocks
            .iter()
            .filter(|range| range.contains(&self.pc))
            .min_by_key(|range| range.len())
            .cloned()
    }

    /// Internal jumps carry no statement; the runner never rests on one.
    fn skip_jumps(&mut self) {
        while let Some(Op::Jump(destination)) =
            self.code.ops.get(self.pc).map(|located| &located.op)
        {
            self.pc = *destination;
        }
    }

    /// The top level has ended. Function bodies after it run only inside
    /// a call.
    pub(in crate::script) fn done(&self) -> bool {
        self.frames.is_empty() && self.pc >= self.code.end
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
        sight: &Sight,
        progress: &mut RunStatus,
        now: Duration,
    ) -> Result<Tick, String> {
        self.execute(&|field| field.read(sight), progress, now)
    }

    /// A run error inside a function names the function and its call.
    fn located(&self, message: String) -> String {
        match self.frames.last() {
            Some(frame) => format!(
                "{message} (in {}, called from line {})",
                self.code.functions[frame.function].name, frame.line
            ),
            None => message,
        }
    }

    /// Evaluate in the running frame, naming a variable read before it is set.
    fn evaluate(
        &self,
        expression: &super::expression::Expr,
        read: &impl Fn(Field) -> Value,
    ) -> Result<Value, String> {
        expression.evaluate(&self.values, read).map_err(|message| {
            if message != super::expression::UNSET {
                return message.to_owned();
            }
            let mut reads = Vec::new();
            expression.reads(&mut reads);
            reads
                .into_iter()
                .find(|index| self.values[*index].is_none())
                .map_or_else(
                    || message.to_owned(),
                    |index| {
                        format!(
                            "'{}' is read before its declaration ran",
                            self.code.variables[index].name
                        )
                    },
                )
        })
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
        let result = self.step(read, progress, now);
        let flow = progress
            .flow
            .as_mut()
            .expect("structured run has flow status");
        self.show(flow);
        result.map_err(|message| self.located(message))
    }

    #[allow(clippy::too_many_lines, reason = "one arm per op")]
    fn step(
        &mut self,
        read: &impl Fn(Field) -> Value,
        progress: &mut RunStatus,
        now: Duration,
    ) -> Result<Tick, String> {
        if self.done() {
            return Ok(Tick::Logic);
        }
        let Some(located) = self.code.ops.get(self.pc) else {
            return Ok(Tick::Logic);
        };
        progress.line = Some(located.line);
        let flow = progress
            .flow
            .as_mut()
            .expect("structured run has flow status");
        // A return is the end of a call, not a statement of its own.
        if !matches!(located.op, Op::Return { .. }) {
            if flow.statements == MAX_EXECUTED {
                return Err(format!(
                    "maximum of {MAX_EXECUTED} executed statements reached"
                ));
            }
            flow.statements += 1;
        }

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
            Op::Store(index, expression) | Op::Assign(index, expression) => {
                let name = self.code.variables[*index].name.clone();
                // A function can assign a global only once its declaration
                // has run, as it can read one.
                if matches!(located.op, Op::Assign(..)) && self.values[*index].is_none() {
                    return Err(format!("'{name}' is assigned before its declaration ran"));
                }
                let value = self.evaluate(expression, read)?;
                self.store(*index, value);
                progress.event(format!("Set {name}"));
                Tick::Logic
            }
            Op::If {
                condition,
                otherwise,
            } => {
                let take = boolean(&self.evaluate(condition, read)?)?;
                flow.branch = Some((located.line, take));
                if !take {
                    self.pc = *otherwise;
                    self.edge = self
                        .code
                        .else_edges
                        .iter()
                        .find(|(entry, _)| *entry == header)
                        .map(|(_, line)| *line);
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
                    repeat_count(&self.evaluate(count, read)?)?
                };
                self.iteration(progress, header, exit, limit, true);
                if let Some(variable) = variable
                    && let Some(entry) = self.loops.last().filter(|entry| entry.header == header)
                {
                    let value = Value::Int(i64::from(entry.number) - 1);
                    self.store(variable, value);
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
                let take = exhausted || boolean(&self.evaluate(condition, read)?)?;
                self.iteration(progress, header, *exit, *limit, take);
                Tick::Logic
            }
            Op::Call {
                function,
                arguments,
            } => {
                let (function, call_line) = (*function, located.line);
                let name = self.code.functions[function].name.clone();
                if self.frames.len() == MAX_DEPTH {
                    return Err(format!(
                        "calls can nest at most {MAX_DEPTH} deep; this call to {name} would be the {}th",
                        MAX_DEPTH + 1
                    ));
                }
                // Arguments are read in the caller, before the callee's
                // slots are set aside.
                let values = arguments
                    .iter()
                    .map(|argument| self.evaluate(argument, read))
                    .collect::<Result<Vec<_>, _>>()?;
                let target = &self.code.functions[function];
                let saved = target
                    .locals
                    .iter()
                    .map(|slot| {
                        (
                            self.values[*slot].take(),
                            std::mem::take(&mut self.summaries[*slot]),
                        )
                    })
                    .collect();
                let (entry, line) = (target.entry, target.line);
                for (slot, value) in target.parameters.clone().into_iter().zip(values) {
                    self.store(slot, value);
                }
                self.frames.push(Frame {
                    function,
                    return_pc: self.pc,
                    line: call_line,
                    saved,
                    loops: std::mem::take(&mut self.loops),
                });
                self.pc = entry;
                // A dot on the `def` line pauses as the call enters.
                self.edge = Some(line);
                self.show_loops(progress);
                progress.event(format!("Call {name}"));
                Tick::Logic
            }
            Op::Return { .. } => {
                let frame = self.frames.pop().expect("a return runs inside a call");
                let target = &self.code.functions[frame.function];
                for (slot, (value, summary)) in target.locals.iter().zip(frame.saved) {
                    self.values[*slot] = value;
                    self.summaries[*slot] = summary;
                }
                // Loops open in the call end with it.
                self.loops = frame.loops;
                self.pc = frame.return_pc;
                let name = target.name.clone();
                self.show_loops(progress);
                progress.event(format!("Return from {name}"));
                Tick::Logic
            }
            Op::Jump(_) => unreachable!("internal jumps are normalized at the statement boundary"),
        };
        // Forward skips and the loop backedge do not use a step or run any code.
        // Every backedge lands on a visible loop header, so chains are bounded.
        self.skip_jumps();
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
    /// slot; one not yet set, such as a loop's that never ran, is empty.
    fn store(&mut self, index: usize, value: Value) {
        let variable: &Variable = &self.code.variables[index];
        self.summaries[index] = format!("{}: {} = {value}", variable.name, variable.kind);
        self.values[index] = Some(value);
    }

    /// The state panel's variables, the running call's first and then the
    /// globals, and the calls in progress, each with its current line.
    fn show(&self, flow: &mut crate::script::execution::FlowStatus) {
        let current = self.frames.last().map(|frame| frame.function);
        let summary = |index: usize| self.summaries[index].clone();
        let mut shown = Vec::new();
        if let Some(function) = current {
            shown.extend(
                self.code.functions[function]
                    .locals
                    .iter()
                    .map(|slot| summary(*slot)),
            );
        }
        shown.extend(
            self.code
                .variables
                .iter()
                .enumerate()
                .filter(|(_, variable)| variable.function.is_none())
                .map(|(index, _)| summary(index)),
        );
        shown.retain(|line| !line.is_empty());
        flow.variables = shown;
        let here = self
            .next_line()
            .or_else(|| self.code.ops.get(self.pc).map(|located| located.line));
        flow.calls = self
            .frames
            .iter()
            .enumerate()
            .map(|(index, frame)| {
                let line = self
                    .frames
                    .get(index + 1)
                    .map(|inner| inner.line)
                    .or(here)
                    .unwrap_or(frame.line);
                (self.code.functions[frame.function].name.clone(), line)
            })
            .collect();
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

    /// Runs a program offline to its end, or its error: the flow status.
    fn finished(source: &str) -> Result<FlowStatus, String> {
        let program = Program::parse(source, Path::new(".")).unwrap();
        let (_, actions, code) = program.into_parts();
        let mut run = Run::new(code.expect("a structured program"), actions);
        let mut status = RunStatus {
            flow: Some(FlowStatus::default()),
            ..RunStatus::default()
        };
        let mut now = Duration::ZERO;
        while !run.done() {
            now += Duration::from_secs(1);
            run.execute(&|_| Value::Unknown, &mut status, now)?;
        }
        Ok(status.flow.unwrap())
    }

    #[test]
    fn functions_run_when_called_before_or_after_their_definition() {
        let source = "pick(1)\ndef pick(n: int):\n    if n == 1:\n        report(\"one\")\n    else:\n        report(\"other\")\npick(2)\n";
        assert_eq!(reports(source).unwrap(), ["one", "other"]);
        // Arguments are expressions, read in the caller.
        let source = "a: int = 3\ndef show(n: int, loud: bool, word: text):\n    if loud and n == 4 and word == \"hi\":\n        report(\"yes\")\ndef outer(n: int):\n    show(n + 1, True, \"hi\")\nouter(a)\n";
        assert_eq!(reports(source).unwrap(), ["yes"]);
        // A function with no parameters, and one with only pass.
        let source =
            "def nothing():\n    pass\ndef hello():\n    report(\"hello\")\nnothing()\nhello()\n";
        assert_eq!(reports(source).unwrap(), ["hello"]);
    }

    #[test]
    fn globals_are_shared_and_locals_belong_to_one_call() {
        let source = "count: int = 0\ndef bump():\n    step: int = 1\n    count += step\nbump()\nbump()\nif count == 2:\n    report(\"two\")\n";
        assert_eq!(reports(source).unwrap(), ["two"]);
        // Each recursive call has its own parameter and loop.
        let source = "def tour(n: int):\n    for i in range(2):\n        report(\"step\")\n        if n > 0:\n            tour(n - 1)\n    if n == 1:\n        report(\"back\")\ntour(1)\n";
        assert_eq!(
            reports(source).unwrap(),
            ["step", "step", "step", "step", "step", "step", "back"]
        );
        // A global declared below the function is unset until its line runs.
        let early =
            "show()\ng: int = 1\ndef show():\n    if g == 1:\n        report(\"set\")\nshow()\n";
        let error = reports(early).unwrap_err();
        assert_eq!(
            error,
            "'g' is read before its declaration ran (in show, called from line 1)"
        );
        let known = "show()\ng: int = 1\ndef show():\n    if known(g):\n        pass\n";
        assert!(
            reports(known)
                .unwrap_err()
                .starts_with("'g' is read before")
        );
        // Assigning it is the same error, and the declaration's value is
        // never overwritten by an earlier call.
        let assigned = "set()\ng: int = 1\ndef set():\n    g = 5\n";
        assert_eq!(
            reports(assigned).unwrap_err(),
            "'g' is assigned before its declaration ran (in set, called from line 1)"
        );
        let compound = "set()\ng: int = 1\ndef set():\n    g += 5\n";
        assert_eq!(reports(compound), reports(assigned));
        let late = "g: int = 1\nshow()\ndef show():\n    if g == 1:\n        report(\"set\")\n";
        assert_eq!(reports(late).unwrap(), ["set"]);
    }

    #[test]
    fn return_leaves_the_call_and_its_loops() {
        let source = "def first():\n    for i in range(5):\n        if i == 2:\n            return\n        report(\"i\")\n    report(\"never\")\nfor j in range(2):\n    first()\nreport(\"after\")\n";
        assert_eq!(reports(source).unwrap(), ["i", "i", "i", "i", "after"]);
        let source =
            "def stop():\n    while True, limit=10:\n        return\nstop()\nreport(\"after\")\n";
        assert_eq!(reports(source).unwrap(), ["after"]);
    }

    #[test]
    fn calls_nest_at_most_eight_deep() {
        let source = "def dive(n: int):\n    if n < 20:\n        dive(n + 1)\ndive(1)\n";
        let error = reports(source).unwrap_err();
        assert!(
            error.starts_with("calls can nest at most 8 deep"),
            "{error}"
        );
        assert!(error.ends_with("(in dive, called from line 3)"), "{error}");
        let source =
            "def dive(n: int):\n    if n < 8:\n        dive(n + 1)\ndive(1)\nreport(\"ok\")\n";
        assert_eq!(reports(source).unwrap(), ["ok"]);
    }

    #[test]
    fn a_call_is_one_statement_and_the_cap_counts_function_bodies() {
        let flow = finished("def f():\n    pass\nf()\nf()\n").unwrap();
        assert_eq!(flow.statements(), 2);
        let flow = finished("def f():\n    n: int = 1\nf()\n").unwrap();
        assert_eq!(flow.statements(), 2);
        let source = "def busy():\n    for i in range(100):\n        pass\nfor j in range(100):\n    busy()\n";
        let error = finished(source).unwrap_err();
        assert!(
            error.starts_with("maximum of 5000 executed statements reached (in busy"),
            "{error}"
        );
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
