
pub trait Solver {
    /// Given a liquidity value, compute the orderbook steps.
    fn steps(&self, liquidity: f64) -> Vec<f64>;
}

// STEP 2: Provide a default implementation.
pub struct DefaultSolver;

impl Solver for DefaultSolver {
    fn steps(&self, liquidity: f64) -> Vec<f64> {
        crate::maths::steps::exponential(liquidity)
    }
}

// pub struct OrderBook<S: Solver = DefaultSolver> {}

// impl<S: Solver> Orderbook<S> {
//     pub fn new(liquidity: f64, solver: S) -> Self {
//         Self { liquidity, solver }
//     }

//     /// Compute the steps using the provided solver.
//     pub fn calculate_steps(&self) -> Vec<f64> {
//         self.solver.steps(self.liquidity)
//     }
// }
