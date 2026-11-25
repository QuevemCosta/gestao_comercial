; Instalador da Gestão Comercial.
; Gere o executável antes: cargo build --release

#define MyAppName "Gestão Comercial"
#define MyAppVersion "1.0.0"
#define MyAppExe "gestao_comercial.exe"

[Setup]
; Mantenha o mesmo AppId em todas as versões para que a atualização substitua a anterior.
AppId={{8F3C2B1E-6D4A-4B7E-9C21-5A0F7E3D9B42}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher=Nome da empresa
; Pasta na raiz do disco C, fora de Program Files: os dados precisam ser gravados pelos usuários.
DefaultDirName=C:\GestaoComercial
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
OutputDir=..\dist
OutputBaseFilename=GestaoComercial-Setup-{#MyAppVersion}
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\{#MyAppExe}
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
; Instalação para todos os usuários, com privilégio de administrador,
; necessária para criar a pasta na raiz do C: e definir suas permissões.
PrivilegesRequired=admin
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "brazilianportuguese"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"

[Tasks]
Name: "desktopicon"; Description: "Criar atalho na área de trabalho"; Flags: unchecked

[Dirs]
; Pasta de dados: os usuários podem criar, alterar e apagar arquivos nela.
; O programa grava aqui o banco, os logs, os backups e as exportações.
Name: "{app}\Dados"; Permissions: users-modify

[Files]
Source: "..\target\release\{#MyAppExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\icon.ico"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExe}"; IconFilename: "{app}\icon.ico"
Name: "{group}\Desinstalar {#MyAppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExe}"; IconFilename: "{app}\icon.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\{#MyAppExe}"; Description: "Abrir {#MyAppName}"; Flags: nowait postinstall skipifsilent

; A pasta {app}\Dados não é removida na desinstalação, porque contém arquivos
; criados pelo programa. O instalador só remove o que ele mesmo instalou.