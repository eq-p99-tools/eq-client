//! A cooperative interpreter. An action is handed off to the ordinary script driver.

use std::time::Duration;

use eq_client_core::world::ClientWorld;

use super::{
    Code, Value,
    compile::{MAX_ITERATIONS, Op},
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
                let summary = format!("{}: {} = {value}", variable.name, variable.kind);
                self.values[*index] = value;
                if *index == flow.variables.len() {
                    flow.variables.push(summary);
                } else {
                    flow.variables[*index] = summary;
                }
                progress.event(format!("Set {}", variable.name));
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
            Op::Repeat { count, exit } => {
                let limit = if let Some(iteration) =
                    self.loops.last().filter(|entry| entry.header == header)
                {
                    iteration.limit
                } else {
                    repeat_count(&count.evaluate(&self.values, read)?)?
                };
                self.iteration(progress, header, *exit, limit, true);
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
                (_, 0, _) => "Repeat count is zero".into(),
                (_, _, Op::While { .. }) => "While iteration limit reached".into(),
                _ => "Repeat finished".into(),
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
        progress
            .flow
            .as_mut()
            .expect("structured run has flow status")
            .loops = self
            .loops
            .iter()
            .map(|entry| (self.code.ops[entry.header].line, entry.number, entry.limit))
            .collect();
        progress.event(event);
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
        Value::Int(_) => Err(format!("repeat count must be from 0 to {MAX_ITERATIONS}")),
        Value::Unknown | Value::Stale => Err(format!("repeat count is {value}")),
        _ => unreachable!("repeat count type was checked during compilation"),
    }
}
