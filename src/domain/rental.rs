use std::collections::HashSet;

use chrono::NaiveDateTime;
use uuid::Uuid;

use super::errors::DomainError;
use super::validation::required_text;
use super::value_objects::Money;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RentalStatus {
    Reserved,
    Active,
    Closed,
    Cancelled,
}

impl RentalStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RentalStatus::Reserved => "reserved",
            RentalStatus::Active => "active",
            RentalStatus::Closed => "closed",
            RentalStatus::Cancelled => "cancelled",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "reserved" => Some(RentalStatus::Reserved),
            "active" => Some(RentalStatus::Active),
            "closed" => Some(RentalStatus::Closed),
            "cancelled" => Some(RentalStatus::Cancelled),
            _ => None,
        }
    }
}

pub fn rental_sequence_name(year: i32) -> String {
    format!("rental-{year}")
}

/// Número legível da locação, por exemplo `L-2026-0003`.
pub fn format_rental_number(year: i32, sequence: i64) -> String {
    format!("L-{year}-{sequence:04}")
}

/// Intervalo `[início, fim)` de uma locação.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Period {
    start: NaiveDateTime,
    end: NaiveDateTime,
}

impl Period {
    pub fn new(start: NaiveDateTime, end: NaiveDateTime) -> Result<Self, DomainError> {
        if end <= start {
            Err(DomainError::InvalidPeriod)
        } else {
            Ok(Self { start, end })
        }
    }

    pub fn restore(start: NaiveDateTime, end: NaiveDateTime) -> Self {
        Self { start, end }
    }

    pub fn start(&self) -> NaiveDateTime {
        self.start
    }

    pub fn end(&self) -> NaiveDateTime {
        self.end
    }

    /// Dois intervalos semiabertos se sobrepõem quando cada início é anterior ao fim do outro.
    pub fn overlaps(&self, start: NaiveDateTime, end: NaiveDateTime) -> bool {
        self.start < end && start < self.end
    }
}

/// Quantidade de um item comprometida em um intervalo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Commitment {
    pub rental_item_id: Uuid,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub quantity: i64,
}

/// Unidades disponíveis de um item no período solicitado.
pub fn available_for(total: i64, commitments: &[Commitment], period: &Period) -> i64 {
    let booked: i64 = commitments
        .iter()
        .filter(|c| period.overlaps(c.start, c.end))
        .map(|c| c.quantity)
        .sum();
    total - booked
}

pub fn ensure_available(available: i64, requested: i64) -> Result<(), DomainError> {
    if requested <= available {
        Ok(())
    } else {
        Err(DomainError::InsufficientAvailability { available, requested })
    }
}

/// Maior demanda simultânea entre os compromissos. Usada para validar baixas de estoque.
pub fn peak_demand(commitments: &[Commitment]) -> i64 {
    commitments
        .iter()
        .map(|point| {
            commitments
                .iter()
                .filter(|c| c.start <= point.start && point.start < c.end)
                .map(|c| c.quantity)
                .sum::<i64>()
        })
        .max()
        .unwrap_or(0)
}

/// Contagem de uma devolução para uma linha da locação.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReturnCount {
    pub returned: i64,
    pub damaged: i64,
    pub lost: i64,
}

/// Entrada de devolução informada pela aplicação.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnEntry {
    pub line_id: Uuid,
    pub count: ReturnCount,
    pub notes: String,
}

/// Registro histórico de uma devolução. Imutável após gravado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnEvent {
    pub id: Uuid,
    pub line_id: Uuid,
    pub occurred_at: NaiveDateTime,
    pub count: ReturnCount,
    pub notes: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RentalLine {
    id: Uuid,
    rental_item_id: Uuid,
    quantity: i64,
    unit_price: Money,
    returned: i64,
    damaged: i64,
    lost: i64,
}

impl RentalLine {
    pub fn new(
        id: Uuid,
        rental_item_id: Uuid,
        quantity: i64,
        unit_price: Money,
    ) -> Result<Self, DomainError> {
        if quantity <= 0 {
            return Err(DomainError::InvalidQuantity);
        }
        if unit_price.is_negative() {
            return Err(DomainError::NegativeValue("valor unitário da locação"));
        }
        Ok(Self {
            id,
            rental_item_id,
            quantity,
            unit_price,
            returned: 0,
            damaged: 0,
            lost: 0,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        id: Uuid,
        rental_item_id: Uuid,
        quantity: i64,
        unit_price: Money,
        returned: i64,
        damaged: i64,
        lost: i64,
    ) -> Self {
        Self { id, rental_item_id, quantity, unit_price, returned, damaged, lost }
    }

    /// Unidades ainda fora da contagem: nem devolvidas, nem danificadas, nem perdidas.
    pub fn outstanding(&self) -> i64 {
        self.quantity - self.returned - self.damaged - self.lost
    }

    pub fn gross(&self) -> Result<Money, DomainError> {
        let quantity = u32::try_from(self.quantity).map_err(|_| DomainError::Overflow)?;
        self.unit_price.checked_mul_qty(quantity)
    }

    fn apply_return(&mut self, count: ReturnCount) -> Result<(), DomainError> {
        if count.returned < 0 || count.damaged < 0 || count.lost < 0 {
            return Err(DomainError::NegativeValue("quantidade devolvida"));
        }
        let movement = count.returned + count.damaged + count.lost;
        if movement == 0 {
            return Err(DomainError::NothingToReturn);
        }
        let outstanding = self.outstanding();
        if movement > outstanding {
            return Err(DomainError::ReturnExceedsOutstanding { outstanding });
        }
        self.returned += count.returned;
        self.damaged += count.damaged;
        self.lost += count.lost;
        Ok(())
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn rental_item_id(&self) -> Uuid {
        self.rental_item_id
    }

    pub fn quantity(&self) -> i64 {
        self.quantity
    }

    pub fn unit_price(&self) -> Money {
        self.unit_price
    }

    pub fn returned(&self) -> i64 {
        self.returned
    }

    pub fn damaged(&self) -> i64 {
        self.damaged
    }

    pub fn lost(&self) -> i64 {
        self.lost
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rental {
    id: Uuid,
    number: String,
    customer_id: Uuid,
    status: RentalStatus,
    period: Period,
    discount: Money,
    lines: Vec<RentalLine>,
    cancelled_reason: Option<String>,
    picked_up_at: Option<NaiveDateTime>,
    closed_at: Option<NaiveDateTime>,
    created_at: NaiveDateTime,
    updated_at: NaiveDateTime,
}

impl Rental {
    pub fn reserve(
        id: Uuid,
        number: String,
        customer_id: Uuid,
        period: Period,
        discount: Money,
        lines: Vec<RentalLine>,
        now: NaiveDateTime,
    ) -> Result<Self, DomainError> {
        if lines.is_empty() {
            return Err(DomainError::EmptyRental);
        }
        ensure_unique_items(&lines)?;
        if discount.is_negative() {
            return Err(DomainError::NegativeValue("desconto da locação"));
        }
        if discount > lines_gross(&lines)? {
            return Err(DomainError::DiscountExceedsAmount);
        }
        Ok(Self {
            id,
            number,
            customer_id,
            status: RentalStatus::Reserved,
            period,
            discount,
            lines,
            cancelled_reason: None,
            picked_up_at: None,
            closed_at: None,
            created_at: now,
            updated_at: now,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn restore(
        id: Uuid,
        number: String,
        customer_id: Uuid,
        status: RentalStatus,
        period: Period,
        discount: Money,
        lines: Vec<RentalLine>,
        cancelled_reason: Option<String>,
        picked_up_at: Option<NaiveDateTime>,
        closed_at: Option<NaiveDateTime>,
        created_at: NaiveDateTime,
        updated_at: NaiveDateTime,
    ) -> Self {
        Self {
            id,
            number,
            customer_id,
            status,
            period,
            discount,
            lines,
            cancelled_reason,
            picked_up_at,
            closed_at,
            created_at,
            updated_at,
        }
    }

    pub fn pick_up(&mut self, now: NaiveDateTime) -> Result<(), DomainError> {
        if self.status != RentalStatus::Reserved {
            return Err(DomainError::RentalNotReserved);
        }
        self.status = RentalStatus::Active;
        self.picked_up_at = Some(now);
        self.updated_at = now;
        Ok(())
    }

    pub fn cancel(&mut self, reason: &str, now: NaiveDateTime) -> Result<(), DomainError> {
        if self.status != RentalStatus::Reserved {
            return Err(DomainError::RentalNotCancellable);
        }
        let reason = required_text("motivo do cancelamento", reason)?;
        self.status = RentalStatus::Cancelled;
        self.cancelled_reason = Some(reason);
        self.updated_at = now;
        Ok(())
    }

    /// Registra devoluções. Quando todas as unidades estiverem contabilizadas, a locação é encerrada.
    pub fn apply_return(&mut self, entries: &[ReturnEntry], now: NaiveDateTime) -> Result<(), DomainError> {
        if self.status != RentalStatus::Active {
            return Err(DomainError::RentalNotActive);
        }
        if entries.is_empty() {
            return Err(DomainError::NothingToReturn);
        }

        // Aplica em uma cópia: se alguma linha falhar, a locação original permanece intacta.
        let mut lines = self.lines.clone();
        for entry in entries {
            let line = lines
                .iter_mut()
                .find(|line| line.id() == entry.line_id)
                .ok_or(DomainError::UnknownRentalLine)?;
            line.apply_return(entry.count)?;
        }

        self.lines = lines;
        self.updated_at = now;
        if self.lines.iter().all(|line| line.outstanding() == 0) {
            self.status = RentalStatus::Closed;
            self.closed_at = Some(now);
        }
        Ok(())
    }

    pub fn ensure_accepts_payments(&self) -> Result<(), DomainError> {
        if self.status == RentalStatus::Cancelled {
            Err(DomainError::RentalCancelled)
        } else {
            Ok(())
        }
    }

    pub fn gross_total(&self) -> Result<Money, DomainError> {
        lines_gross(&self.lines)
    }

    pub fn total(&self) -> Result<Money, DomainError> {
        self.gross_total()?.checked_sub(self.discount)
    }

    pub fn outstanding_units(&self) -> i64 {
        self.lines.iter().map(RentalLine::outstanding).sum()
    }

    /// Atraso é derivado: locação retirada, com devolução vencida e unidades em aberto.
    pub fn is_late(&self, now: NaiveDateTime) -> bool {
        self.status == RentalStatus::Active && now > self.period.end() && self.outstanding_units() > 0
    }

    /// Compromissos que esta locação impõe à disponibilidade.
    /// Locação retirada e vencida mantém os itens ocupados até a devolução.
    pub fn commitments(&self, now: NaiveDateTime) -> Vec<Commitment> {
        if !matches!(self.status, RentalStatus::Reserved | RentalStatus::Active) {
            return Vec::new();
        }
        let end = if self.status == RentalStatus::Active && now > self.period.end() {
            NaiveDateTime::MAX
        } else {
            self.period.end()
        };
        self.lines
            .iter()
            .filter(|line| line.outstanding() > 0)
            .map(|line| Commitment {
                rental_item_id: line.rental_item_id(),
                start: self.period.start(),
                end,
                quantity: line.outstanding(),
            })
            .collect()
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn number(&self) -> &str {
        &self.number
    }

    pub fn customer_id(&self) -> Uuid {
        self.customer_id
    }

    pub fn status(&self) -> RentalStatus {
        self.status
    }

    pub fn period(&self) -> Period {
        self.period
    }

    pub fn discount(&self) -> Money {
        self.discount
    }

    pub fn lines(&self) -> &[RentalLine] {
        &self.lines
    }

    pub fn cancelled_reason(&self) -> Option<&str> {
        self.cancelled_reason.as_deref()
    }

    pub fn picked_up_at(&self) -> Option<NaiveDateTime> {
        self.picked_up_at
    }

    pub fn closed_at(&self) -> Option<NaiveDateTime> {
        self.closed_at
    }

    pub fn created_at(&self) -> NaiveDateTime {
        self.created_at
    }

    pub fn updated_at(&self) -> NaiveDateTime {
        self.updated_at
    }
}

fn ensure_unique_items(lines: &[RentalLine]) -> Result<(), DomainError> {
    let mut seen = HashSet::with_capacity(lines.len());
    for line in lines {
        if !seen.insert(line.rental_item_id()) {
            return Err(DomainError::DuplicateRentalItem);
        }
    }
    Ok(())
}

fn lines_gross(lines: &[RentalLine]) -> Result<Money, DomainError> {
    lines
        .iter()
        .try_fold(Money::ZERO, |acc, line| acc.checked_add(line.gross()?))
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;

    fn dt(day: u32, hour: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 11, day)
            .and_then(|date| date.and_hms_opt(hour, 0, 0))
            .expect("data de teste válida")
    }

    #[test]
    fn period_must_end_after_start() {
        assert_eq!(Period::new(dt(21, 8), dt(21, 8)), Err(DomainError::InvalidPeriod));
        assert!(Period::new(dt(21, 8), dt(21, 9)).is_ok());
    }

    #[test]
    fn touching_periods_do_not_overlap() -> Result<(), DomainError> {
        let period = Period::new(dt(21, 8), dt(21, 20))?;
        assert!(!period.overlaps(dt(21, 20), dt(22, 8)));
        assert!(period.overlaps(dt(21, 19), dt(22, 8)));
        Ok(())
    }

    #[test]
    fn peak_demand_sums_simultaneous_commitments() {
        let item = Uuid::now_v7();
        let commitments = [
            Commitment { rental_item_id: item, start: dt(21, 8), end: dt(21, 20), quantity: 3 },
            Commitment { rental_item_id: item, start: dt(21, 12), end: dt(21, 14), quantity: 2 },
            Commitment { rental_item_id: item, start: dt(22, 8), end: dt(22, 9), quantity: 4 },
        ];
        assert_eq!(peak_demand(&commitments), 5);
    }

    #[test]
    fn late_active_rental_keeps_item_booked_indefinitely() -> Result<(), DomainError> {
        let line = RentalLine::new(Uuid::now_v7(), Uuid::now_v7(), 2, Money::from_cents(500))?;
        let mut rental = Rental::reserve(
            Uuid::now_v7(),
            "L-2026-0001".to_owned(),
            Uuid::now_v7(),
            Period::new(dt(21, 8), dt(21, 20))?,
            Money::ZERO,
            vec![line],
            dt(16, 9),
        )?;
        rental.pick_up(dt(21, 8))?;

        let commitments = rental.commitments(dt(22, 10));
        assert_eq!(commitments.len(), 1);
        assert_eq!(commitments[0].end, NaiveDateTime::MAX);
        assert!(rental.is_late(dt(22, 10)));
        Ok(())
    }
}