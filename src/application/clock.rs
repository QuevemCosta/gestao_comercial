use chrono::{Local, NaiveDateTime};

/// Fonte de tempo injetável, para que os testes controlem as datas.
pub trait Clock {
    fn now(&self) -> NaiveDateTime;
}

pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> NaiveDateTime {
        Local::now().naive_local()
    }
}