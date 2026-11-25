use thiserror::Error;

use super::value_objects::Money;

/// Erros de regra de negócio. Não conhecem banco de dados nem interface.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DomainError {
    #[error("operação numérica excedeu o limite permitido")]
    Overflow,
    #[error("o campo '{0}' é obrigatório")]
    EmptyField(&'static str),
    #[error("o campo '{0}' não pode ser negativo")]
    NegativeValue(&'static str),
    #[error("a quantidade deve ser maior que zero")]
    InvalidQuantity,
    #[error("estoque insuficiente: disponível {available}, solicitado {requested}")]
    InsufficientStock { available: i64, requested: i64 },
    #[error("a contagem física informada não pode ser negativa")]
    NegativeCount,
    #[error("a contagem informada é igual ao saldo atual; nenhum ajuste necessário")]
    NoStockChange,
    #[error("produto inativo não pode receber movimentações nem ser incluído em pedidos")]
    ProductInactive,
    #[error("CPF ou CNPJ inválido")]
    InvalidTaxId,
    #[error("o documento informado não corresponde ao tipo de pessoa (CPF para pessoa física, CNPJ para jurídica)")]
    TaxIdKindMismatch,
    #[error("telefone inválido; informe DDD e número")]
    InvalidPhone,
    #[error("informe pelo menos um telefone ou WhatsApp")]
    ContactRequired,
    #[error("cliente inativo não pode receber novos pedidos ou locações")]
    CustomerInactive,
    #[error("pedidos para entrega exigem endereço de entrega")]
    DeliveryAddressRequired,
    #[error("somente pedidos em elaboração podem ser alterados, confirmados ou cancelados")]
    OrderNotDraft,
    #[error("o desconto não pode ser maior que o valor a que se aplica")]
    DiscountExceedsAmount,
    #[error("o produto já está incluído neste pedido")]
    DuplicateProduct,
    #[error("não é possível confirmar um pedido sem itens")]
    EmptySale,
    #[error("somente vendas confirmadas podem ser concluídas ou canceladas")]
    SaleNotConfirmed,
    #[error("venda cancelada não aceita pagamentos")]
    SaleCancelled,
    #[error("o valor do pagamento deve ser maior que zero")]
    NonPositivePayment,
    #[error("o pagamento excede o saldo pendente de {balance}")]
    PaymentExceedsBalance { balance: Money },
    #[error("o período da locação deve terminar depois do início")]
    InvalidPeriod,
    #[error("a locação precisa de pelo menos um item")]
    EmptyRental,
    #[error("o mesmo item não pode aparecer duas vezes na mesma locação")]
    DuplicateRentalItem,
    #[error("este item de locação está inativo")]
    RentalItemInactive,
    #[error("somente locações reservadas podem ser retiradas")]
    RentalNotReserved,
    #[error("somente locações retiradas podem registrar devoluções")]
    RentalNotActive,
    #[error("somente locações reservadas podem ser canceladas")]
    RentalNotCancellable,
    #[error("locação cancelada não aceita pagamentos")]
    RentalCancelled,
    #[error("informe ao menos uma quantidade devolvida, danificada ou perdida")]
    NothingToReturn,
    #[error("a devolução ultrapassa as unidades em aberto ({outstanding})")]
    ReturnExceedsOutstanding { outstanding: i64 },
    #[error("a linha informada não pertence a esta locação")]
    UnknownRentalLine,
    #[error("disponibilidade insuficiente no período: disponível {available}, solicitado {requested}")]
    InsufficientAvailability { available: i64, requested: i64 },
    #[error("a baixa não pode ultrapassar o total atual de {total} unidades")]
    WriteOffExceedsTotal { total: i64 },
    #[error("a baixa deixaria reservas sem capacidade; total mínimo exigido: {required}")]
    WriteOffBlockedByReservations { required: i64 },
    #[error("somente pedidos com entrega podem gerar entrega")]
    DeliveryNotRequired,
    #[error("a entrega só pode ser criada para pedido confirmado")]
    OrderNotConfirmed,
    #[error("venda cancelada não permite entrega")]
    CancelledSaleNoDelivery,
    #[error("este pedido já possui uma entrega")]
    DeliveryAlreadyExists,
    #[error("a data da entrega não pode estar no passado")]
    DeliveryDateInPast,
    #[error("transição de entrega não permitida: de '{from}' para '{to}'")]
    InvalidDeliveryTransition {
        from: &'static str,
        to: &'static str,
    },
    #[error("a entrega precisa de um entregador atribuído antes de sair")]
    DeliveryNotAssigned,
    #[error("entrega encerrada; não é possível alterá-la")]
    DeliveryClosed,
    #[error("entrega em andamento: registre o resultado antes de cancelar a venda")]
    DeliveryInProgress,
    #[error("entrega já concluída: o cancelamento da venda deve ser tratado manualmente")]
    DeliveryAlreadyCompleted,
    #[error("entregador inativo não pode receber entregas")]
    DelivererInactive,
    #[error("o documento de entregador deve ser um CPF")]
    DelivererDocumentMustBeCpf,
    #[error("a entrega já está atribuída a este entregador")]
    AlreadyAssignedToDeliverer,
    #[error("esta atribuição já foi encerrada")]
    AssignmentClosed,
}

/// Erros reportados pelos contratos de persistência. A infraestrutura os produz;
/// o domínio apenas os define.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RepositoryError {
    #[error("registro não encontrado")]
    NotFound,
    #[error("conflito: {0}")]
    Conflict(String),
    #[error(transparent)]
    Rule(#[from] DomainError),
    #[error("falha de armazenamento: {0}")]
    Storage(String),
}