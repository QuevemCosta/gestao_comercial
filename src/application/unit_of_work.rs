use crate::domain::report_repository::ReportRepository;
use crate::domain::repositories::{
    AuditRepository, CategoryRepository, CustomerRepository, DelivererRepository,
    DeliveryRepository, OrderRepository, PaymentRepository, ProductRepository,
    RentalItemRepository, RentalRepository, SaleRepository, SequenceRepository, StockRepository,
};

use super::error::AppError;

/// Unidade de trabalho: as escritas feitas por ela só são persistidas em `commit`.
/// Se a unidade for descartada sem commit (inclusive em caso de erro), nada é gravado.
pub trait UnitOfWork:
    CategoryRepository
    + ProductRepository
    + StockRepository
    + CustomerRepository
    + OrderRepository
    + SaleRepository
    + PaymentRepository
    + SequenceRepository
    + RentalItemRepository
    + RentalRepository
    + AuditRepository
    + DelivererRepository
    + DeliveryRepository
    + ReportRepository
{
    fn commit(self: Box<Self>) -> Result<(), AppError>;
}

/// Porta que abre unidades de trabalho. A implementação concreta fica na infraestrutura.
pub trait Transactor {
    fn begin(&mut self) -> Result<Box<dyn UnitOfWork + '_>, AppError>;
}