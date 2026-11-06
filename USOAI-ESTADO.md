# usoai — versión de prueba 1.24.1 para Windows

Base: vinzdg/codenotch v1.24.0 (6522e9f3ee0fe35d46e428b441e6e80b586eacbb).
Rama local: usoai/main. No se subió código ni se cambiaron los remotos.

## Resultado entregado

Instalador: usoai-Setup.exe, compilación x64 de Windows, aproximadamente 3.4 MB.
Esta es una primera versión de prueba del alcance prioritario (Claude, Codex y OpenCode). El plan completo todavía no está terminado.

## Implementado

- Identidad usoai, datos en %APPDATA%/usoai, puerto propio y actualizaciones del original deshabilitadas.
- Modelo común de cuentas y migración de perfiles Claude, con guardado atómico de configuración.
- Pantalla de cuentas en español e inglés: añadir, renombrar, pausar, conectar y quitar cuentas adicionales. Las cuentas predeterminadas se pueden pausar.
- Claude y Codex: alta mediante el inicio de sesión oficial en una carpeta independiente, o registro de una carpeta existente.
- Claude: estado de inicio de sesión y renovación separado por perfil.
- Codex: consulta de uso, recuperación mediante app-server y lectura de sesiones con el CODEX_HOME de cada cuenta.
- OpenCode: varias carpetas de credenciales, API key manual o variable de entorno.
- Claves manuales en Windows Credential Manager, bajo el espacio usoai. No se guardan claves en JSON ni se devuelven a la pantalla.
- Detección acotada de carpetas conocidas y variables del proceso; el usuario selecciona qué candidatos añadir.
- Lecturas, cachés, plazo mínimo de consulta y reintentos separados por cuenta de Codex y OpenCode.
- Indicadores separados en notch, bandeja y barra de tareas para las cuentas configuradas. Las cuentas adicionales no heredan animaciones de actividad de otra cuenta.
- Al quitar una cuenta adicional se conserva la carpeta del proveedor. Si falla el guardado de configuración, se intenta restaurar la clave propia que se estuviera quitando.
- Workflow de GitHub Actions preparado para construir el instalador sin publicar en el proyecto original.

## Verificación realizada

- Rust: 326 pruebas pasaron, 0 fallaron, 4 ignoradas. Incluyen migración de configuración, almacenamiento de una clave ficticia en Windows, aislamiento de cuentas y persistencia.
- JavaScript: 14 scripts de pruebas pasan, además de la comprobación de sintaxis de todas las páginas y scripts.
- Qoder: su script de prueba sigue bloqueado por EPERM al lanzar procesos desde Node en este entorno. No se atribuye a un fallo del proveedor y no se ha modificado su integración.
- Interfaz revisada con cuentas ficticias: renderizado, renombrado, pausa, alta de una clave de ejemplo, vaciado del campo de clave y ausencia de esa clave en el HTML. Sin desbordamiento horizontal en el panel.
- La automatización del diálogo de confirmación de eliminación no pudo completarse; necesita comprobación manual.
- Instalador NSIS generado. Prueba de instalación en una carpeta de trabajo, sin accesos directos: código de salida 0; aplicación y hook presentes. La aplicación instalada coincide con la compilada tras normalizar el marcador NSIS documentado de Tauri; el hook coincide exactamente.
- Una primera prueba de desinstalación retiró la aplicación y el hook con salida 0. La integración completa con el registro de programas de Windows no se pudo verificar dentro del entorno restringido.
- Se confirmó que una cuenta real de Codex está conectada y recibe lecturas de consumo. La presentación de esas lecturas en la pantalla de cuentas se corrigió en 1.24.1; la verificación de la actualización instalada está en curso.
- No se instaló una copia de producción en la carpeta habitual del usuario. La instalación normal y la apertura siguen pendientes de la prueba del usuario.

## Lo que falta del documento original

- Probar instalación normal, apertura y consumo con cuentas reales; comprobar manualmente quitar una cuenta.
- Completar hooks y atribución de actividad para todos los perfiles Claude, incluidas carpetas personalizadas y cambios de cuentas mientras está abierta la aplicación.
- Detección desde HKCU\Environment y detección de todos los demás proveedores.
- Extraer y mostrar correo y plan como metadatos estructurados por cuenta.
- Migrar los demás proveedores a varias cuentas; ahora conservan su integración original de cuenta única.
- Pulido: agrupación configurable por cuenta/proveedor, ocultar agotadas, exportación/importación de listas y diagnóstico por cuenta.
- macOS queda como fase opcional posterior.
- Firma de código y actualizador propios; el actualizador está deshabilitado.

## Cómo probar la versión

1. Abrir usoai-Setup.exe e instalar para el usuario actual.
2. Abrir usoai. Acceder a Ajustes desde el icono de la bandeja o el engranaje.
3. En Cuentas, conectar Claude o Codex con el CLI oficial instalado. Para una cuenta adicional, pulsar Añadir cuenta, elegir proveedor, nombre e inicio de sesión oficial.
4. Para OpenCode, registrar su carpeta de datos o añadir una clave de OpenCode Go.
5. Completar personalmente el inicio de sesión oficial. No enviar contraseñas ni tokens en el chat.
6. Confirmar que las lecturas y los alias de dos cuentas del mismo proveedor son independientes.

La captura usoai-cuentas-ejemplo.jpg usa datos ficticios. No muestra las cuentas del usuario.

Se conserva la licencia MIT original. Los nombres internos de los ejecutables son codenotch.exe y codenotch-hook.exe.

## Corrección 1.24.1 — consumo en Ajustes → Cuentas

La primera pantalla de cuentas solo mostraba el estado y la fecha de consulta, aunque el motor ya recibía límites. La respuesta list_accounts ahora incluye las ventanas de consumo. La interfaz muestra porcentajes usados y disponibles, una barra y la renovación de cada límite. Las lecturas antiguas se distinguen explícitamente; no se inventa un 100 % disponible si falta información. Las lecturas se actualizan sin interrumpir un cambio de nombre. También se elimina el aviso de espera de inicio de sesión cuando ya hay una lectura correcta.

Pruebas: 326 Rust correctas y 6 pruebas del módulo de presentación de cuentas correctas, incluida la regresión de porcentajes visibles. Sintaxis de todas las páginas verificada. Instalador usoai-1.24.1-Setup.exe compilado. Se guardó una copia local de configuración antes de la actualización; no se copiaron credenciales de los proveedores.

La versión instalada del equipo ya se confirmó como 1.24.1. Se verificó que se conservaron los perfiles configurados y que una cuenta Codex real sigue recibiendo consumo. Se confirmó en la aplicación instalada que Ajustes → Cuentas muestra usado, disponible y renovación para una cuenta real de Codex.

Verificación final de 1.24.1: versión instalada confirmada, 12 perfiles configurados conservados sin cambios y consumo visible en la pantalla de cuentas, contrastado con la lectura real almacenada por el proveedor. La revisión se realizó mediante el árbol de accesibilidad de la aplicación en ejecución.
