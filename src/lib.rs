use std::time::Duration;

use serde::{Deserialize, Serialize};
use worker::{
    console_error, durable_object, Date, DurableObject, Env, Error, Headers, Request, Response,
    State,
};

#[durable_object]
pub struct RustyLimiter {
    kv_key: &'static str,
    state: State,
    cooldown_in_ms: u64,
    max_reqs: u64,
}

#[derive(Deserialize, Serialize)]
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
            kv_key: "rate-limit",
            state: state,
            cooldown_in_ms: cooldown_in_ms,
            max_reqs: max_reqs,
        }
    }

    async fn alarm(&self) -> Result<Response, Error> {
        match self.state.storage().delete(self.kv_key).await {
            Ok(_) => {}
            Err(e) => console_error!(
                "error reseting rate-limit {} for {}",
                e,
                self.state.id().to_string()
            ),
        };
        Response::empty()
    }

    async fn fetch(&self, _req: Request) -> Result<Response, Error> {
        match self.state.storage().get_alarm().await {
            Ok(alarm) => match alarm {
                Some(_) => {}
                None => {
                    let storage = self.state.storage();
                    let cooldown = self.cooldown_in_ms;
                    self.state.wait_until(async move {
                        let sch = Duration::from_millis(cooldown);
                        match storage.set_alarm(sch).await {
                            Ok(_) => {}
                            Err(e) => console_error!("error creating alarm {}", e),
                        };
                    });
                }
            },
            Err(e) => console_error!("error retrieving alarm {}", e),
        }

        let storage = self.state.storage();
        let kv_key = self.kv_key;
        match self.state.storage().get::<RateLimitRows>(self.kv_key).await {
            Ok(entry) => match entry {
                Some(mut val) => {
                    val.counter += 1;

                    let now = Date::now().as_millis();
                    let cooldown = self.cooldown_in_ms;

                    if val.counter >= self.max_reqs && val.next_allowed_time < now {
                        let now_duration = Duration::from_millis(now);
                        if val.next_allowed_time == 0 {
                            val.next_allowed_time =
                                (now_duration + Duration::from_millis(cooldown)).as_secs();
                        }

                        let retry_after = now_duration
                            .abs_diff(Duration::from_secs(val.next_allowed_time))
                            .as_secs()
                            % 60;

                        self.state.wait_until(async move {
                            match storage.put(kv_key, &val).await {
                                Ok(_) => {}
                                Err(e) => {
                                    console_error!("error when calling put on storage: {}", e)
                                }
                            };
                        });

                        let headers = Headers::new();
                        headers.set("Retry-After", &(retry_after).to_string())?;

                        let mut response = Response::error("Rate limited", 429)?;
                        response = response.with_headers(headers);

                        return Ok(response);
                    } else {
                        self.state.wait_until(async move {
                            match storage.put(kv_key, &val).await {
                                Ok(_) => {}
                                Err(e) => {
                                    console_error!("error when calling put on storage: {}", e)
                                }
                            };
                        });
                    }
                }
                None => {
                    let val = RateLimitRows {
                        next_allowed_time: 0,
                        counter: 1,
                    };

                    self.state.wait_until(async move {
                        match storage.put(kv_key, &val).await {
                            Ok(_) => {}
                            Err(e) => {
                                console_error!("error when calling put on storage: {}", e)
                            }
                        };
                    });
                }
            },
            Err(e) => {
                console_error!("error retrieving KV entry {}", e);
                return Err(e);
            }
        }

        Response::empty()
    }
}
