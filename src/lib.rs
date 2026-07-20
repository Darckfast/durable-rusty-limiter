use std::time::Duration;

use serde::Deserialize;
use wasm_bindgen::JsValue;
use worker::{
    console_error, durable_object, Date, DurableObject, Env, Error, Headers, Request, Response,
    State,
};

#[durable_object]
pub struct RustyLimiter {
    state: State,
    cooldown_in_ms: u64,
    max_reqs: u64,
}

#[derive(Deserialize)]
struct RateLimitRows {
    next_allowed_time: u64,
    counter: u64,
}

impl DurableObject for RustyLimiter {
    fn new(state: State, env: Env) -> Self {
        let cooldown_in_ms: u64 = match env.var("COOLDOWN_IN_MS") {
            Ok(a) => a.to_string().parse().unwrap_or(60),
            Err(_) => 60u64,
        };
        let max_reqs: u64 = match env.var("MAX_REQS") {
            Ok(a) => a.to_string().parse().unwrap_or(10),
            Err(_) => 10u64,
        };

        Self {
            state: state,
            cooldown_in_ms: cooldown_in_ms,
            max_reqs: max_reqs,
        }
    }

    async fn alarm(&self) -> Result<Response, Error> {
        // Durable Object only ceases to exists if, when it shuts down and its storage is empty
        // including alarms.
        // https://developers.cloudflare.com/durable-objects/best-practices/access-durable-objects-storage/#remove-a-durable-objects-storage
        self.state
            .storage()
            .delete_all()
            .await
            .inspect_err(|e| console_error!("error deleting storage from limiter: {e:?}"))?;
        self.state
            .storage()
            .delete_alarm()
            .await
            .inspect_err(|e| console_error!("error deleting alarm from limiter: {e:?}"))?;

        Response::empty()
    }

    async fn fetch(&self, _req: Request) -> Result<Response, Error> {
        let storage = self.state.storage();
        let cooldown = self.cooldown_in_ms;

        let _ = storage.sql().exec(
            r#"CREATE TABLE IF NOT EXISTS ratelimit (
        "id" INTEGER PRIMARY KEY AUTOINCREMENT,
        "next_allowed_time" INTEGER NOT NULL DEFAULT 0,
        "counter" INTEGER NOT NULL DEFAULT 0
        )"#,
            None,
        )?;

        let ratelimit = storage
            .sql()
            .exec_raw(
                "INSERT INTO ratelimit (id, next_allowed_time, counter) VALUES (1,0,1) 
            ON CONFLICT (id) DO UPDATE SET counter = (counter + 1), next_allowed_time = CASE 
                WHEN counter >= ? THEN (unixepoch('subsec') * 1000) + ?
                ELSE 0
            END RETURNING id, next_allowed_time, counter",
                vec![
                    JsValue::from(self.max_reqs as usize),
                    JsValue::from(self.cooldown_in_ms as usize),
                ],
            )
            .inspect_err(|e| console_error!("error retrieving 'ratelimit' entry: {e:?}"))?
            .one::<RateLimitRows>()
            .inspect_err(|e| console_error!("error parsing 'ratelimit' result: {e:?}"))?;

        self.state.wait_until(async move {
            match storage.get_alarm().await {
                Ok(alarm) => match alarm {
                    Some(_) => {}
                    None => {
                        let cooldown = Duration::from_millis(cooldown);
                        match storage.set_alarm(cooldown).await {
                            Ok(_) => {}
                            Err(e) => console_error!("error creating alarm {e:?}"),
                        };
                    }
                },
                Err(e) => console_error!("error retrieving alarm {e:?}"),
            }
        });

        if ratelimit.counter >= self.max_reqs {
            let now = Duration::from_millis(Date::now().as_millis());

            let retry_after = now
                .abs_diff(Duration::from_millis(ratelimit.next_allowed_time))
                .as_secs()
                % 60;

            let headers = Headers::new();
            headers.set("Retry-After", &(retry_after).to_string())?;

            let mut response = Response::error("Rate limited", 429)?;
            response = response.with_headers(headers);

            Ok(response)
        } else {
            Response::empty()
        }
    }
}
