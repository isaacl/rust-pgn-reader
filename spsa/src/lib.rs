extern crate rand;

use rand::XorShiftRng;
use rand::distributions::{IndependentSample, Range};

pub struct HyperParameters {
    alpha: f64,
    gamma: f64,
    theta: [f64; 4],
    a_par: f64,
    noise_var: f64,
}

impl Default for HyperParameters {
    fn default() -> HyperParameters {
        HyperParameters {
            alpha: 0.7,
            gamma: 0.101,
            theta: [0.0; 4],
            a_par: 0.7,
            noise_var: 1.8,
        }
    }
}

impl HyperParameters {
    pub fn spsa(&self) -> Spsa {
        assert!(self.noise_var > 0.0);

        Spsa {
            rng: XorShiftRng::new_unseeded(),
            k: 0.0,
            alpha: self.alpha,
            gamma: self.gamma,
            theta: self.theta,
            a_par: self.a_par,
            noise_var: self.noise_var,
        }
    }
}

pub struct Spsa {
    rng: XorShiftRng,
    k: f64,
    alpha: f64,
    gamma: f64,
    theta: [f64; 4],
    a_par: f64,
    noise_var: f64,
}

impl Spsa {
    pub fn step<F>(&mut self, loss: &mut F)
        where F: FnMut([f64; 4]) -> f64
    {
        let _old_theta = self.theta;

        // need tweaking
        let ak = self.a_par / (self.k + 1.0 + 100.0).powf(self.alpha);
        let ck = self.noise_var / (self.k + 1.0).powf(self.gamma);

        let mut ghat = [0.0; 4];

        let ens_size = 3;
        let range = Range::new(-4, 5);
        let mut delta = [0.0; 4];

        let mut byte_guess = 0.0;

        for _ in 0..ens_size {

            for i in 0..4 {
                delta[i] = ck * f64::from(range.ind_sample(&mut self.rng));
            }

            let mut theta_plus = self.theta;
            let mut theta_minus = self.theta;
            for i in 0..4 {
                theta_plus[i] += delta[i];
                theta_minus[i] -= delta[i];
            }

            let j_plus = loss(theta_plus);
            let j_minus = loss(theta_minus);

            byte_guess += j_plus + j_minus;

            for i in 0..4 {
                if delta[i] != 0.0 {
                    ghat[i] += (j_plus - j_minus) / (2.0 * delta[i]);
                }
            }
        }

        for i in 0..4 {
            self.theta[i] -= ak * ghat[i];
        }

        // let mut sum = 0.0;
        // for i in 0..4 {
        //     sum += self.theta[i] * self.theta[i];
        // }
        // sum = 1.0 / sum.sqrt();
        // for i in 0..4 {
        //     self.theta[i] *= sum;
        // }

        println!("k={:03} bytes={:.2} theta=[{}]", self.k, byte_guess / f64::from(2 * ens_size),
                 // UGH
                 self.theta.iter().map(|v| format!("{:.3}", v)).collect::<Vec<_>>().join(", "));

        self.k += 1.0;
    }

    pub fn theta(&self) -> [f64; 4] {
        self.theta
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let mut loss = |x: [f64; 4]| {
            x[0].powi(2) +
            x[1].powi(2) +
            x[2].powi(2) +
            x[3].powi(2)
        };

        let mut spsa = HyperParameters::default().spsa();

        for i in 0..2000 {
            println!("{}: {:?}", i, spsa.theta());
            spsa.step(&mut loss);
        }

        assert!(false);
    }
}
