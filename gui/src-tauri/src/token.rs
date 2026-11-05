//! One-shot apply tokens (design §8.2 item 5). A token comes from the last
//! successful preview of one plan file (same name and `plan_id`) in this
//! session, expires after five minutes or at any refresh, and is consumed by
//! the apply it authorizes.

use std::time::{Duration, Instant, SystemTime};

pub const LIFETIME: Duration = Duration::from_secs(300);

#[derive(Debug)]
struct Entry {
    plan: String,
    plan_id: String,
    token: String,
    issued: Instant,
    /// `Instant` stops while the Mac sleeps; the wall clock does not.
    wall: SystemTime,
}

impl Entry {
    fn expired(&self) -> bool {
        self.issued.elapsed() >= LIFETIME || self.wall.elapsed().map_or(true, |e| e >= LIFETIME)
    }
}

#[derive(Debug, Default)]
pub struct Tokens(Vec<Entry>);

impl Tokens {
    /// Replaces any token for the same plan file.
    pub fn issue(&mut self, plan: &str, plan_id: &str) -> String {
        self.0.retain(|e| e.plan != plan);
        let token = uuid::Uuid::new_v4().to_string();
        self.0.push(Entry {
            plan: plan.to_owned(),
            plan_id: plan_id.to_owned(),
            token: token.clone(),
            issued: Instant::now(),
            wall: SystemTime::now(),
        });
        token
    }

    /// Removes the token; succeeds only if it was issued for this plan file
    /// and `plan_id` and has not expired.
    pub fn consume(&mut self, plan: &str, plan_id: &str, token: &str) -> Result<(), String> {
        let found = self.0.iter().position(|e| e.token == token);
        let entry = found.map(|i| self.0.remove(i));
        match entry {
            Some(e) if e.plan == plan && e.plan_id == plan_id && !e.expired() => Ok(()),
            Some(e) if e.expired() => Err("预览已超过 5 分钟，请重新预览".into()),
            _ => Err("预览已失效（界面已刷新或计划已变化），请重新预览".into()),
        }
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_one_shot_and_bound_to_the_plan() {
        let mut tokens = Tokens::default();
        let t = tokens.issue("a.json", "sha256:1");
        assert!(tokens.consume("b.json", "sha256:1", &t).is_err());
        let t = tokens.issue("a.json", "sha256:1");
        assert!(tokens.consume("a.json", "sha256:2", &t).is_err());
        let t = tokens.issue("a.json", "sha256:1");
        assert!(tokens.consume("a.json", "sha256:1", &t).is_ok());
        assert!(tokens.consume("a.json", "sha256:1", &t).is_err());
        let t = tokens.issue("a.json", "sha256:1");
        tokens.0[0].wall -= LIFETIME;
        assert!(
            tokens
                .consume("a.json", "sha256:1", &t)
                .unwrap_err()
                .contains("5 分钟")
        );
    }
}
