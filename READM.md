# Gestão Comercial

Sistema de gestão comercial para desktop (Windows) com funcionamento local e offline.
Controla produtos, estoque, clientes, pedidos, vendas, pagamentos, locação de mesas
e cadeiras, entregas e entregadores, além de relatórios gerenciais.

## Requisitos

- Windows 10 ou 11, 64 bits.
- Nenhuma instalação adicional: o banco de dados (SQLite) e a interface estão incluídos no executável.

## Instalação

1. Copie `gestao_comercial.exe` para uma pasta de sua escolha, por exemplo `C:\GestaoComercial`.
2. Dê dois cliques no arquivo para abrir o sistema.

Na primeira execução, o sistema cria o banco de dados e as pastas abaixo.

## Onde ficam os dados

| O quê | Local |
|---|---|
| Banco de dados | `%LOCALAPPDATA%\GestaoComercial\data\gestao_comercial.db` |
| Logs | `%LOCALAPPDATA%\GestaoComercial\data\logs\` |
| Backups | `%LOCALAPPDATA%\GestaoComercial\data\backups\` |
| Exportações (CSV) | `%LOCALAPPDATA%\GestaoComercial\data\exportacoes\` |

Para abrir a pasta rapidamente, pressione `Windows + R`, digite `%LOCALAPPDATA%\GestaoComercial` e pressione Enter.

Todos os caminhos aparecem na tela **Configurações**.

## Backup e restauração

### Criar um backup
1. Abra **Configurações**.
2. Clique em **Criar backup agora**.

O backup é uma cópia completa e consistente do banco, com data e hora no nome. Faça backups regularmente e guarde uma cópia fora do computador (pendrive ou nuvem).

### Restaurar um backup
1. Feche o sistema, se estiver aberto.
2. Abra o sistema e vá em **Configurações**.
3. Escolha o backup na lista.
4. Marque **Entendo que os dados atuais serão substituídos**.
5. Clique em **Preparar restauração**.
6. Feche e abra o sistema novamente.

A troca só acontece na inicialização. O banco anterior é guardado na mesma pasta com o sufixo `.antes-restauracao-<data>`, para que nada seja perdido. Se a restauração falhar, o sistema registra o erro no log e continua com o banco atual.

## Logs

Os logs ficam em `%LOCALAPPDATA%\GestaoComercial\data\logs\` e são criados um por dia. Em caso de problema, envie o arquivo do dia ao suporte.

## Interface

A interface inclui o painel, produtos, categorias, estoque, clientes, pedidos, vendas, pagamentos, itens de locação, locações, entregas, entregadores, relatórios e configurações.

A interface é construída com [Slint](https://slint.dev). A tela inicial exibe o componente de atribuição da Slint, conforme os termos de licença vigentes.

## Desenvolvimento

Pré-requisitos: Rust (via `rustup`) e Visual Studio Build Tools com o componente "Desenvolvimento para desktop com C++".

```bash
cargo build              # compilação de desenvolvimento
cargo test               # testes unitários e de integração
cargo run                # executa em modo de desenvolvimento
cargo build --release    # gera target\release\gestao_comercial.exe
```

### Estrutura

- `src/domain`: regras de negócio, sem dependência de banco ou interface.
- `src/application`: casos de uso, executados dentro de unidades de trabalho (transações).
- `src/infrastructure`: SQLite, migrações, backup, logs e configuração de caminhos.
- `src/presentation`: interface Slint e controlador, que apenas traduz ações da tela em casos de uso.
- `migrations/`: esquema do banco, versionado por arquivo (`001` a `006`).
- `tests/`: testes de integração com SQLite em memória.

### Migrações

Migrações já aplicadas nunca devem ser editadas. Mudanças no esquema são feitas sempre em um novo arquivo, acrescentado à lista `MIGRATIONS` em `src/infrastructure/migrations.rs`.