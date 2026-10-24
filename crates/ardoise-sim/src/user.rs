//! The person in front of the simulated tablet, for the guided test.

use ardoise_device::diagnostic::{Decision, Instruction, Operator, Progress, StepId, StepOutcome};
use ardoise_proto::stroke::Stroke;

use crate::drawings;
use crate::tablet::SimTablet;

/// Performs the physical actions the guided test asks for on a [`SimTablet`]: draws the
/// reference pages on [`Instruction::DrawReferencePages`], holds the button on
/// [`Instruction::HoldButtonToRegister`], presses it on [`Instruction::PressButtonToConfirm`]
/// and [`Instruction::PressButton`], and queues live handwriting on [`Instruction::DrawLive`].
///
/// Alone ([`SimulatedUser::autonomous`]) it answers every prompt with
/// [`Decision::Continue`]. Wrapping another operator (the interface), it forwards every call
/// to it first and acts only when the answer is `Continue` (instructions shown without a
/// question, [`Operator::notify`], are always acted upon).
pub struct SimulatedUser {
    tablet: SimTablet,
    inner: Option<Box<dyn Operator + Send>>,
    pages: Vec<Vec<Stroke>>,
    actions: Vec<String>,
}

impl SimulatedUser {
    /// A simulated user who does everything asked, without asking anyone.
    pub fn autonomous(tablet: SimTablet) -> Self {
        let config = tablet.config();
        Self {
            pages: drawings::reference_pages(config.width, config.height),
            tablet,
            inner: None,
            actions: Vec::new(),
        }
    }

    /// A simulated user who acts only once `inner` agreed.
    pub fn wrapping(tablet: SimTablet, inner: Box<dyn Operator + Send>) -> Self {
        Self {
            inner: Some(inner),
            ..Self::autonomous(tablet)
        }
    }

    /// Draws these pages (cycled) instead of [`drawings::reference_pages`].
    pub fn with_pages(mut self, pages: Vec<Vec<Stroke>>) -> Self {
        self.pages = pages;
        self
    }

    /// What the simulated user did so far, one line per action.
    pub fn actions(&self) -> &[String] {
        &self.actions
    }

    pub fn tablet(&self) -> &SimTablet {
        &self.tablet
    }

    fn act(&mut self, instruction: &Instruction) {
        match instruction {
            Instruction::DrawReferencePages { count } => {
                for index in 0..*count {
                    let page = usize::try_from(index)
                        .ok()
                        .and_then(|i| self.pages.get(i % self.pages.len().max(1)));
                    if let Some(page) = page {
                        self.tablet.draw_page(page);
                    }
                }
                self.actions.push(format!("drew {count} reference pages"));
            }
            Instruction::HoldButtonToRegister => {
                self.tablet.hold_button();
                self.actions.push("held the button".into());
            }
            Instruction::PressButtonToConfirm | Instruction::PressButton => {
                self.tablet.press_button();
                self.actions.push("pressed the button".into());
            }
            Instruction::DrawLive { seconds } => {
                let config = self.tablet.config();
                self.tablet
                    .draw_live(drawings::live_script(*seconds, config.width, config.height));
                self.actions.push(format!("drew live for {seconds} s"));
            }
        }
    }
}

impl Operator for SimulatedUser {
    fn started(&mut self, step: StepId) {
        if let Some(inner) = &mut self.inner {
            inner.started(step);
        }
    }

    fn prompt(&mut self, step: StepId, instruction: &Instruction) -> Decision {
        let decision = match &mut self.inner {
            Some(inner) => inner.prompt(step, instruction),
            None => Decision::Continue,
        };
        if decision == Decision::Continue {
            self.act(instruction);
        }
        decision
    }

    fn notify(&mut self, step: StepId, instruction: &Instruction) {
        if let Some(inner) = &mut self.inner {
            inner.notify(step, instruction);
        }
        self.act(instruction);
    }

    fn progress(&mut self, step: StepId, progress: &Progress) {
        if let Some(inner) = &mut self.inner {
            inner.progress(step, progress);
        }
    }

    fn finished(&mut self, outcome: &StepOutcome) {
        if let Some(inner) = &mut self.inner {
            inner.finished(outcome);
        }
    }

    fn cancelled(&self) -> bool {
        self.inner.as_ref().is_some_and(|inner| inner.cancelled())
    }
}
