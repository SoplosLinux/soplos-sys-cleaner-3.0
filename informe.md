# Informe de auditoría — Soplos Sys Cleaner 3.0 vs 2.x vs Kudu

Fecha: 2026-10-09 · Alcance: lectura de código fuente de los tres proyectos. **No se ha modificado, compilado ni ejecutado nada**; este fichero es lo único que se ha escrito.

| Proyecto | Ruta | Stack | Versión |
|---|---|---|---|
| Sys Cleaner 3.0 | `soplos-sys-cleaner-3.0` | Tauri 2 (Rust) + Svelte 4 + Tailwind 3 | 3.0.0-beta |
| Sys Cleaner 2.x | `soplos-sys-cleaner` | Python 3 + GTK3, helper root persistente por JSON | 1.0.3-4 |
| Kudu | `kudu` | Electron + React 19 + Tailwind 4 + Zustand, reglas JSON | 3.6.1 (MIT) |

---

## 1. Resumen ejecutivo

1. **3.0 es un buen esqueleto, no un sustituto de 2.x todavía.** Tiene 8 categorías de limpieza de las ~13 pestañas de 2.x, y le faltan precisamente las herramientas que hacen único a Sys Cleaner en Soplos: **kernels, drivers por hardware, firmware, idiomas/docs, DKMS y referencias huérfanas de módulos, autoremove, reconstrucción de initrd**.
2. **Problema de diseño prioritario (§1.1): solo Resumen parece un producto.** Papelera, Caché APT y Logs son páginas enteras con una tarjeta de una cifra, y el resto son listas planas bajo un banner con foto. Los intentos previos solo agrandaron tarjetas. Hay que dar a **cada pestaña** la densidad de Resumen mediante una plantilla de página con secciones obligatorias (stats, análisis, lista rica, contexto, actividad, acción) antes de añadir más herramientas.
3. **La estética de 3.0 ya imita a Kudu a medias** (tarjetas, anillo de salud, estadísticas, acciones rápidas, actividad reciente, almacenamiento). Lo que no tiene es lo que da a Kudu su sensación de producto: sistema de tokens CSS completo, shell con titlebar, sidebar agrupada con submenús, cabecera de página con pasos, flujo único *escanear → revisar → confirmar → recibo*, y ajuste de tema "Sistema".
4. **Hay dos problemas de seguridad que conviene corregir antes de añadir nada más** (§4.1): inyección de comandos como root a través de nombres de archivo en `/tmp`, y un mecanismo polkit que equivale a "ejecutar cualquier script como root".
5. **Las estadísticas pueden mentir**: el código ignora el resultado de los trabajos privilegiados y registra "espacio recuperado" aunque el usuario cancele la contraseña o el comando falle (§4.2).
6. **Recomendación:** mantener Tauri + Svelte (no portar a Electron/React), portar de Kudu el *diseño y los patrones* —no el código— y su *formato de reglas JSON* (MIT, compatible con GPL-3.0+ citando copyright), y reconstruir las herramientas de 2.x como módulos con tests, empezando por los de menor riesgo. Plan por fases en §7.

---

## 1.1 Problema prioritario: pestañas enormes con una tarjeta pobre

**Es el defecto de diseño más visible de 3.0 y es prioridad de Fase 1, al nivel de la seguridad.**

**Qué hay hoy**
- 10 pestañas. **Papelera, Caché APT y Logs** son una página entera cuyo único contenido es `StatHero.svelte`: un icono, un título, un número y un botón en una fila. El resto de la pantalla está vacío.
- El resto (Caché, Flatpak, Snap, Apps, Temp) son una lista plana de casillas con una barra fija inferior, sin icono por elemento, sin descripción, sin indicación de seguridad.
- Sobre todas, un banner de ~100 px con foto que repite lo que dice la sidebar.
- **Solo Resumen se ve como un producto**: anillo de salud, 3 estadísticas, accesos, actividad reciente, almacenamiento. Las demás pestañas parecen de otra aplicación.

**Por qué el agente solo "agranda tarjetas"**
Cada pedido de "que se vea mejor" se ha resuelto escalando el contenedor (`p-7`, `text-[2.2rem]`, `w-16 h-16`, como en `StatHero`) y no añadiendo información. Una tarjeta con un dato no mejora por ser más grande: lo que falta es **contenido**. La solución no es un componente más grande, sino una **plantilla de página** con secciones obligatorias, y una regla de revisión: *si una pestaña tiene un solo dato, está mal diseñada*.

**Regla de diseño (a dar al agente literalmente)**
> Cada pestaña debe tener la misma densidad y calidad que Resumen. Prohibido aumentar el tamaño de una tarjeta para llenar espacio. Si falta espacio, se añade contenido útil según la plantilla de abajo. Máximo de altura de una tarjeta de una sola cifra: la de las `StatCard` de Resumen.

### Plantilla de página rica (aplicar a TODAS las pestañas)

1. **Cabecera compacta** (≈56 px, estilo Kudu): kicker con icono, título, descripción de una línea y botón primario a la derecha ("Escanear" / "Limpiar X"). Sin banner de foto.
2. **Fila de 3-4 `StatCard`** (misma que Resumen): reclamable ahora, elementos encontrados, total recuperado históricamente en esta categoría, último escaneo/limpieza.
3. **Panel de análisis**: gráfico o desglose de dónde está el espacio (barras horizontales ordenadas, anillo por tipo, o distribución por antigüedad). Nunca solo un número.
4. **Lista/tabla rica** con búsqueda, orden, filtro y "solo seleccionados"; cada fila con icono, nombre, descripción corta, etiqueta de seguridad (Seguro / Revisar / Requiere contraseña), tamaño con barra proporcional y antigüedad.
5. **Panel lateral o inferior de contexto**: qué hace esta limpieza, qué se conserva, qué pasa si se borra, y consejos ("Se regenera al abrir la app").
6. **Actividad reciente de esa categoría** (últimas 5 limpiezas con fecha y bytes) y enlace a Historial.
7. **Barra de acción sticky** única con resumen ("12 elementos · 1,8 GB") y botón primario.
8. **Estados cuidados**: escaneo (esqueleto + progreso), vacío ("Todo limpio" con check y fecha), error con reintento, éxito con resumen y deshacer cuando sea posible.

### Contenido concreto por pestaña actual

| Pestaña | Hoy | Contenido rico propuesto |
|---|---|---|
| Papelera | 1 tarjeta | Stats (elementos, tamaño, más antiguo, recuperado histórico) · desglose por tipo (imágenes, documentos, vídeo, comprimidos, otros) · lista de los 20 más grandes con vista previa de nombre/origen/fecha de borrado · restaurar individual o vaciar · papeleras de otras particiones (`.Trash-1000`) |
| Caché de usuario | lista plana | Treemap/barras por app con icono real de la app (leído de su `.desktop`) · etiqueta "se regenera / pierde sesión / shaders lentos de recompilar" · stats y actividad · exclusiones |
| Caché APT | 1 tarjeta | Stats (nº de .deb, tamaño, listas APT, antigüedad) · lista de .deb individuales con paquete/versión/fecha · paquetes huérfanos (autoremove con simulación) · aviso de qué hace `apt clean` |
| Logs | 1 tarjeta | Gráfico de uso de journald por servicio/prioridad · entradas de `/var/log` ordenadas · selector de retención (7/30 días o tamaño) · estimación de lo que se liberaría antes de ejecutar |
| Temporales | lista plana | Distribución por antigüedad (1-7 d, 7-30 d, +30 d) y por propietario · filas con tipo (carpeta/archivo) y sugerencia de seguridad · explicación de qué se excluye y por qué |
| Flatpak | lista | Runtimes sin uso vs. en uso (anillo) · apps con su icono, versión y tamaño · datos de usuario (`~/.var/app`) · tamaño total de Flatpak |
| Snap | lista | Revisiones antiguas agrupadas por snap, con revisión activa marcada · cuánto ocupa cada snap · ayuda sobre `refresh.retain` |
| Apps instaladas | lista | Icono real, versión, tamaño, fecha de instalación, "última vez usada" si hay datos · filtros (con lanzador, librerías, más grandes) · simulación de dependencias antes de desinstalar |
| Historial | lista de 20 | Gráfico de espacio recuperado por semana/mes · filtro por categoría · recibos expandibles por elemento · exportar |

El mismo criterio vale para las pestañas que se restauren de 2.x (Kernels, Drivers, Firmware, Idiomas): deben nacer ya con esta plantilla, no como lista de casillas.

### Unificación sin perder riqueza
- La sidebar sigue teniendo una entrada por herramienta *con contenido propio* (todas lo tendrán con la plantilla). Para evitar crecer a 15 entradas, agrupar con **grupos plegables y submenús** (patrón Kudu): Limpieza · Paquetes y sistema · Disco · Actividad.
- Se añade una vista **Limpiador** (Kudu) como atajo global "escanear todo → revisar → limpiar", sin sustituir las pestañas especializadas.
- Quitar el banner de foto y duplicar de la barra inferior (`fixed left-[230px]`) por un componente `ActionBar` único.

### Cómo medir que está bien (criterios de aceptación)
1. Ninguna pestaña tiene menos de **cuatro bloques de información** distintos (stats, análisis, lista, contexto/actividad).
2. Ninguna tarjeta de una cifra supera la altura de las `StatCard` de Resumen; ninguna página tiene más de ~25 % de espacio vacío a 1100×720.
3. Capturas de cada pestaña lado a lado con Resumen deben parecer la misma aplicación.
4. Revisión visual en claro y oscuro antes de dar por cerrada cada pestaña.
5. Cada PR de UI incluye capturas antes/después; se rechaza cualquier cambio cuyo único efecto sea agrandar un componente.

---

## 2. Comparativa funcional

Leyenda: ✅ completo · 🟡 parcial · ❌ no existe · — no aplica

### 2.1 Limpieza y paquetes

| Capacidad | 2.x | 3.0 | Kudu (Linux) |
|---|---|---|---|
| Papelera de usuario | ✅ | ✅ | ✅ |
| `~/.cache` por carpeta (selección) | ✅ | ✅ | ✅ (reglas por app, mucho más granular) |
| Caché de navegadores / apps / GPU / Steam / juegos | ❌ | ❌ (solo `~/.cache` bruto) | ✅ reglas JSON `browsers`, `apps`, `gpu-cache`, `steam`, `gaming` |
| `/tmp` y `/var/tmp` (con edad mínima) | ✅ | ✅ (solo 1.er nivel, ≥1 día) | ✅ |
| Caché APT: tamaño y `apt clean` | ✅ | ✅ (todo o nada) | ✅ |
| Caché APT: **.deb individuales** | ✅ | ❌ | — |
| Listas APT (`/var/lib/apt/lists`) | ✅ (tamaño) | ❌ | — |
| `apt autoremove` con vista previa | ✅ | ❌ | — |
| journald | ✅ (`--vacuum-size`) | 🟡 (fijo 7 días) | ✅ (30 d / 1 GB) |
| `/var/log` por entrada | ✅ | ❌ | 🟡 |
| Crash reports (`/var/crash`) | ✅ | ❌ | ✅ |
| Flatpak runtimes sin uso | ✅ | ✅ | ✅ + caches `~/.var/app` |
| Snap revisiones antiguas | ✅ | ✅ | ✅ + caches |
| Apps instaladas manualmente (desinstalar) | ✅ (purge + autoremove + initrd) | 🟡 (`apt-get remove` simple) | ✅ Uninstaller con restos |
| **Kernels antiguos** (+ headers/src/kbuild/dirs huérfanos) | ✅ | ❌ | — |
| **Drivers innecesarios por hardware** (GPU, WiFi, VM guest, impresoras…) | ✅ | ❌ | 🟡 (Driver Manager, orientado a actualizar) |
| **Firmware por familias con protección de hardware real** | ✅ | ❌ | — |
| **Idiomas y documentación** (localepurge, GNOME Help, KDE) | ✅ | ❌ | — |
| **DKMS huérfanos** y **referencias huérfanas** (`/etc/modules`, `modules-load.d`, `dracut.conf.d`) | ✅ | ❌ | — |
| Reconstrucción de initrd (`dracut -f`) / `update-grub` | ✅ | ❌ | — |

### 2.2 Aplicación

| Capacidad | 2.x | 3.0 | Kudu |
|---|---|---|---|
| Resumen del sistema (variante Soplos, kernel, sesión, uptime) | ✅ | 🟡 (solo DE + protocolo) | ✅ |
| Monitor de recursos (CPU, RAM, disco, temp, GPU) | ✅ | ❌ (solo uso de discos) | ✅ |
| Dashboard con estadísticas persistentes | ❌ | ✅ | ✅ |
| Limpieza rápida / completa en un clic | ❌ ("limpiar todo" por pestaña) | ✅ | ✅ |
| Historial | ❌ | 🟡 (20 entradas, sin detalle) | ✅ **recibos** por elemento (borrado / omitido / fallido) |
| Vista previa antes de borrar | ✅ | ✅ (listas con casillas) | ✅ |
| Detección de procesos que bloquean (p. ej. navegador abierto) | ❌ | ❌ | ✅ |
| Exclusiones del usuario | ❌ | ❌ | ✅ |
| Búsqueda / orden / filtro en resultados | ❌ | ❌ | ✅ |
| Programación automática + bandeja | ❌ | ❌ | ✅ (daemon + tray) |
| CLI sin GUI (`--cli --json`) | ❌ | ❌ | ✅ |
| Analizador de disco, archivos grandes, duplicados, carpetas vacías | ❌ | ❌ | ✅ |
| Gestor de inicio / servicios | ❌ | ❌ | ✅ |
| Onboarding | ❌ | ❌ | ✅ |
| Idiomas | 8 | 8 (en/es/de/fr/it/pt/ro/ru) | 30 |
| Tema | sigue al escritorio (detección por DE) | claro/oscuro manual, **oscuro por defecto** | Sistema / Claro / Oscuro |
| Modelo de privilegios | sesión root persistente (modo Administrador) | `pkexec` por operación (sin modo Admin) | elevación por relanzado |
| Tests | ❌ | ❌ | ✅ vitest (cientos) |

**Lectura:** 3.0 aporta lo nuevo (dashboard, rápida/completa, historial, Tauri ligero, `pkexec` por operación). 2.x aporta lo irremplazable (gestión de kernel/driver/firmware/idioma/DKMS). Kudu aporta el modelo de producto y el motor de reglas.

---

## 3. Qué hace Kudu que merece copiarse (y qué no)

### 3.1 Estética ("Calm Control")
- **Tokens CSS** completos y nombrados por función, no por color: `--page-bg`, `--card-bg`, `--bg-hover/-active/-elevated/-overlay`, `--text-primary/-secondary/-muted/-dim`, `--border-subtle…-stronger`, `--accent`, `--accent-muted-bg/-border`, `--nav-active-bg/-fg`, `--glass-*`, `--gauge-track`, semánticos `--success/-warning/-danger/-info`, y la variante clara (`.light`). 3.0 tiene solo 7 variables (`--color-dark/panel/surface/surface-2/border/text*`).
- **Shell**: titlebar propia con marca + menú de apariencia (Sistema/Claro/Oscuro), sidebar con grupos y submenús flotantes, banner de administrador, área de contenido con `skip-nav` (accesibilidad) y soporte RTL.
- **Cabecera de página** (`PageHeader`): *kicker* con icono y familia, título, descripción y **pasos 01·02·03** del flujo. Sustituye al banner con foto de 3.0 (`App.svelte`, imagen `sys-cleaner-background.jpg`).
- **Componentes base**: `HealthScore` (anillo con color por umbral: ≥71 verde, ≥41 ámbar, resto rojo), `StatCard` con contador animado y variantes, `SegmentedMeter`, `ConfirmDialog`, `EmptyState`, `ErrorAlert`, `ScanProgress`, `CleanSummary`, toasts (`sonner`).
- **Dashboard orientado a objetivos** (espacio / velocidad / protección → herramientas recomendadas) en lugar de una cuadrícula de accesos.
- Nota: el `globals.css` de Kudu apila tres capas de tokens (ámbar original → "Calm Control" verde → claro cálido). **No copiarlo literalmente**; tomar solo la capa final y reducirla.

### 3.2 Funcionalidad
- **Motor de reglas declarativo** (`rules/linux/*.json`, esquema JSON, `needsAdmin`, `cleanupAction` con identificadores fijos, variables `${HOME}`, `${CACHE}`, `${TMPDIR}`). Las reglas no pueden aportar comandos: solo acciones de una lista cerrada (`journal-vacuum`, `uv cache prune`…). Es justo la defensa que 3.0 necesita contra la inyección (§4.1).
- **Flujo único de limpieza**: escanear todo → categorías → resultados agrupados con buscar/ordenar/"solo seleccionados" → comprobación de bloqueos → confirmación → **recibo** por elemento → historial.
- Acciones nativas **no seleccionadas por defecto** y con "ahorro variable" (no inventan bytes).
- Programación, daemon/bandeja, CLI, exclusiones, restauración de ajustes (recovery), onboarding.

### 3.3 Qué NO traer
Registro, menú contextual, debloater, malware/YARA, CVE, firewall audit, breach monitor, Cloud/Pro (tiers), cualquier cosa Windows/macOS, Game Mode (valorar más adelante). Tampoco Electron/React: Tauri pesa una fracción y ya está integrado con `pkexec`/polkit de Soplos.

### 3.4 Licencia
Kudu es **MIT**; Sys Cleaner es **GPL-3.0-or-later**. Se puede incorporar código/reglas MIT en un proyecto GPL conservando el aviso de copyright y la licencia MIT en un `THIRD_PARTY`/`NOTICE`. Si solo se reproduce el *diseño* (paleta, estructura) con código propio, no hay obligación, pero conviene citarlo.

---

## 4. Auditoría de 3.0

### 4.1 Seguridad (corregir primero)

**S1 — ALTA — Inyección de comandos como root vía nombre de archivo en `/tmp`.**
`src-tauri/src/commands/temp_files.rs:86-90`: `clean_temp_files` valida solo el prefijo `/tmp/`|`/var/tmp/`, escapa únicamente `"` y construye `rm -rf -- "<ruta>"` que se ejecuta con `sh -c` como root (`jobs.rs`, `run_privileged_script_file`). Dentro de comillas dobles el shell sigue expandiendo `$(…)`, `` `…` `` y `$VAR`.
Escenario: cualquier usuario local crea `/tmp/x$(comando)` y le pone una fecha antigua (`touch -d '2 days ago'`); el escáner lo lista (≥1 día), la víctima pulsa *Limpieza completa* y `comando` se ejecuta como root. Además `/tmp/../etc/…` pasa el filtro `starts_with`.
Arreglo: no pasar rutas por shell. Que el helper privilegiado reciba rutas por argv o fichero, las **canonicalice**, exija que sean hijas directas de `/tmp` o `/var/tmp`, rechace symlinks y borre desde Rust (`remove_dir_all`) o con `rm -rf -- "$@"` (sin interpolar). Mismo patrón defensivo en `flatpak.rs:171-180` (la lista negra omite `\`; mejor lista blanca de caracteres como ya hace `apps.rs`).

**S2 — ALTA/MEDIA — La acción polkit es un "ejecuta cualquier script como root".**
`resources/polkit/org.soplos.syscleaner.policy` autoriza el binario `/usr/bin/soplos-sys-cleaner` con `auth_admin_keep`; `main.rs` acepta `--run-privileged-script <ruta>` y `run_privileged_script_file` ejecuta **cualquier** fichero con `sh -c`. Tras una autenticación, cualquier proceso del mismo usuario puede invocar `pkexec /usr/bin/soplos-sys-cleaner --run-privileged-script /ruta/mia` sin nueva contraseña durante el periodo "keep". El mensaje de polkit ("scan or clean system files") no refleja esa capacidad.
Además `jobs.rs:110-116` crea el script en `/tmp` con nombre predecible (pid + contador) y **aplica `0600` después de crearlo** (ventana de carrera, `File::create` con umask).
Arreglo: sustituir "script libre" por **operaciones con nombre** (`--op apt-clean`, `--op journal-vacuum --days 7`, `--op delete-temp <rutas>`, `--op snap-remove <nombre> <rev>`…) que el binario root valida y ejecuta él mismo. Crear ficheros temporales con `OpenOptions::mode(0o600).create_new(true)` en `$XDG_RUNTIME_DIR`. Es el mismo diseño que usa Kudu con sus `cleanupAction` cerrados.

**S3 — MEDIA — Desinstalar apps sin simulación.** `apps.rs:121` ejecuta `apt-get remove -y` con los nombres elegidos; apt puede arrastrar paquetes dependientes (metapaquetes de escritorio, `soplos-*`). No hay `-s` previo ni lista de paquetes protegidos. 2.x al menos usaba purge + autoremove con reconstrucción de initrd.
Arreglo: `apt-get -s remove` para mostrar el árbol real en el modal; lista de protegidos (kernel en uso, `soplos-*`, metapaquetes de DE, `pkexec`/polkit); bloquear si la simulación elimina un protegido.

**S4 — BAJA** — `tauri.conf.json`: `"csp": null`. La app no carga contenido remoto, pero conviene un CSP estricto (`default-src 'self'`). `capabilities/default.json`: la descripción dice "Soplos WebApp Manager" (copiada de otra app) y concede `shell:allow-open`; revisar si se usa.

### 4.2 Corrección funcional

**F1 — MEDIA — El resultado de los trabajos se ignora.** `lib/job.ts` devuelve `success`, pero **ningún llamador lo comprueba**: `Overview.svelte:163-189`, `TempView`, `LogsView`, `AptCacheView`, `FlatpakView`, `SnapView`, `AppsView` llaman `record_cleanup` siempre. Si el usuario cancela `pkexec` o el comando falla, "Espacio recuperado" y el historial se inflan. El propio comentario de `stats.rs` promete "nada sintetizado".
Arreglo: registrar solo si `success`; mostrar error y salida del job; en *Limpieza completa*, continuar o abortar según política y reflejar fallos en el resumen. (El recibo por elemento de Kudu resuelve esto de forma natural.)

**F2 — MEDIA — Limpieza rápida/completa sin vista previa.** Vacía **todo** `~/.cache` y la papelera tras un único aviso (`Overview.svelte`, `doQuickClean`), sin exclusiones ni detección de apps abiertas (navegador, IDE, caches de shaders que cuestan recompilar). Y borra la caché de **apps en ejecución**. Texto de la tarjeta: "al instante, sin pedir contraseña".
Arreglo: paso intermedio *Escanear → Revisar → Limpiar* (el patrón Kudu), exclusiones persistentes, comprobar procesos bloqueantes.

**F3 — BAJA** — `record_scan` se ejecuta al abrir Resumen aunque solo se escanean 3 categorías; "Último escaneo" no significa lo que dice. `stats.json` guarda *claves i18n* como `label` (`"overview.trash.title"`): renombrar una clave rompe el historial; se escribe sin atomicidad (`fs::write`) y sin versión de esquema; `HistoryView` promete "todas las limpiezas" pero `MAX_ACTIVITY = 20`.

**F4 — BAJA** — Flatpak: el escaneo usa `flatpak uninstall --unused` (ambos ámbitos) pero la limpieza corre como root y solo ve el ámbito sistema; las referencias de usuario pueden no limpiarse (verificar en un equipo con apps Flatpak de usuario). `ref_size_bytes` solo busca bajo `runtime/`.

**F5 — BAJA** — journald fijo a 7 días (2.x permitía tamaño); sin configuración. Temp: solo primer nivel, no hay forma de ver por qué algo se excluye.

### 4.3 Calidad de código y mantenimiento
- `dir_size_bytes` está **copiado 6 veces** en Rust (trash, user_cache, flatpak, snap, temp_files…) y lanza un proceso `du` por entrada. Mover a `utils` (y valorar un recorrido nativo con `walkdir`/`jwalk` cacheado).
- La lista seleccionable con barra inferior `fixed left-[230px]` está **duplicada en 5 vistas** (Cache, Apps, Temp, Flatpak, Snap). Extraer `SelectableList.svelte` + `ActionBar.svelte`; hoy un cambio de ancho de sidebar rompe cinco barras a mano.
- `i18n/index.ts:37,48` usa `localStorage` sin `try/catch` (a diferencia de `theme.ts`); lanza si el almacenamiento está bloqueado. 8 ficheros de 178 líneas: no hay comprobación automática de paridad de claves.
- Sin tests de ningún tipo (los parsers de `flatpak`, `snap`, `journalctl`, `df` son el sitio obvio); sin CI; **no es un repositorio git**; sin `.gitignore`, `README`, `CHANGELOG`, `CLAUDE.md`.
- Los 8 cmd de escaneo devuelven `unwrap_or_default()` ante error: un fallo real (`du` ausente, panic del hilo) se ve como "nada que limpiar". Falta un canal de error visible.
- Los `Overview` escanean secuencialmente (`await` en cadena); paralelizar.

### 4.4 UX
- Sin progreso ni salida en vivo del trabajo (`job.ts` descarta la salida que el backend ya transmite línea a línea).
- Sin buscador/ordenación, sin descripción por elemento, sin tamaño relativo salvo en Caché.
- Tema oscuro fijo por defecto; no sigue al escritorio (2.x sí). Sin opción "Sistema".
- Banner con foto fija ocupa ~100 px en todas las páginas y no aporta información.
- Atajos: F1, Ctrl+Q, Ctrl+Tab (coherentes con otras apps Soplos); falta foco/ARIA en la lista y en la sidebar (Kudu tiene `skip-nav`, roles, RTL).

### 4.5 Empaquetado
- `bundle.targets: ["deb"]`, depende solo de `pkexec`; el binario además necesita `apt-mark`, `dpkg-query`, `dpkg`, `du`, `df`, `journalctl` y, opcionalmente, `flatpak`/`snap`. Declarar `Depends`/`Recommends`.
- Carpetas `icons/ios` y `icons/android` sin uso (limpieza de repositorio).
- El paquete .deb se llama igual que el de 2.x (`soplos-sys-cleaner`): planificar `Replaces/Conflicts/Breaks` y la ruta de actualización. La política polkit apunta a `/usr/bin/soplos-sys-cleaner`; en desarrollo (`target/debug/…`) el prompt no coincidirá con la acción.

### 4.6 Lo que 3.0 hace bien (conservar)
- Separación clara por módulo (`commands/*.rs`) y `spawn_blocking` en todos los escaneos; el hilo UI nunca se bloquea.
- Validaciones defensivas ya presentes: `delete_cache_entries` solo acepta hijos directos de `~/.cache`; `/tmp` ignora sockets, FIFOs, symlinks y `systemd-private-*`; exclusión de extensiones Flatpak GL/ffmpeg/VAAPI en uso.
- `single-instance`, workaround `WEBKIT_DISABLE_DMABUF_RENDERER` en VM, perfil `release` con LTO/strip, sin `devtools` en release.
- Política de "números reales": el diseño pretende no fabricar métricas; solo hay que cerrar el hueco F1.
- Dashboard, Quick/Full Clean, almacenamiento real vía `df` filtrando `/dev/*`, i18n con `Intl.RelativeTimeFormat`.

---

## 5. Lo que 2.x sabe y 3.0 debe heredar (conocimiento acumulado)

El `CHANGELOG` de 2.x es un catálogo de errores caros ya resueltos. Al portar cada módulo **hay que convertir cada entrada en un test**:

| Módulo | Lección que no puede perderse |
|---|---|
| Kernels | Igualdad exacta, no subcadena (`6.1.0-2` ⊂ `6.1.0-27`). Ignorar `-dbg` y metapaquetes. Proteger kernel en uso y kernels Soplos (`bore-ntsync`). Limpiar `/usr/src` y `/lib/modules` huérfanos. |
| Firmware | Un módulo cargado **no implica** hardware: `nouveau`/`iwlwifi` en una VM listan firmware de todos los chips. Solo fiarse de módulos ligados a dispositivo real (`/sys/module/<m>/drivers/`). Intel WiFi exige PCI clase *network*, no cualquier `8086`. Familias dentro de `firmware-linux-nonfree` son "temporales" (vuelven al actualizar). |
| Drivers | Detección en tres capas (lspci + lsusb + lsmod/modinfo). Deduplicar `spice-*` entre `kvm`/`qemu`. `virt-viewer/virt-manager` son clientes remotos: no marcarlos. Purgar el stack completo antes de borrar su firmware. |
| Referencias huérfanas | `/etc/modules`, `modules-load.d`, `dracut.conf.d` (los instaladores de NVIDIA escriben ahí sin dpkg). Exclusión dura de `soplos.conf`, `i18n.conf`, `local.conf`. Comentarios `# módulo` mal parseados (`lstrip('#').strip()`). VirtualBox Guest Additions: ejecutar `uninstall.sh`/`rcvboxadd cleanup`. |
| DKMS | Falsos positivos con kernels Soplos (prefijo `kernel-`) si hay `v4l2loopback-dkms` o `nvidia-driver`. |
| Concurrencia | `dracut -f` y `autoremove` no deben solaparse (hubo corrupción de protocolo). |
| Locales | `localepurge` + `/etc/locale.nopurge` con los idiomas a conservar; proteger el locale activo + inglés. |
| Tema | Detección por escritorio (XFCE `xfconf-query`, Plasma `kdeglobals`, GNOME `color-scheme`) ya escrita y reutilizable para la opción "Sistema". |

Debilidades de 2.x que **no** deben repetirse: `delete_paths` borra cualquier ruta que reciba el helper root (sin validación), sesión root persistente, y ausencia de tests.

---

## 6. Brecha de estética: 3.0 → Kudu (con identidad Soplos)

Decisión de partida propuesta: **estructura y tokens de Kudu, acento Soplos** (naranja `#ff7d2e` / `#fec431`), manteniendo paleta neutra de 3.0 salvo que se prefiera el verde oscuro de Kudu (§8, pregunta 1).

| Elemento | 3.0 hoy | Objetivo (Kudu) |
|---|---|---|
| Tokens | 7 variables RGB | ~45 tokens semánticos, 2 temas (+ "Sistema") |
| Fondo | liso | `--page-bg` + brillos ambientales sutiles opcionales |
| Tarjetas | `bg-surface border` | `glass-*` / `--card-bg` con sombra suave y hover elevado |
| Sidebar | plana, 230 px, grupos con etiqueta | grupos colapsables + submenús flotantes, `--nav-active-bg`, marca arriba |
| Titlebar | decoración nativa | nativa al principio; titlebar propia opcional (ver riesgos: GNOME/Wayland) |
| Cabecera de página | foto + título | kicker + `h1` + descripción + pasos 01/02/03 |
| Dashboard | cuadrícula de accesos | anillo de salud + 3 objetivos (Espacio / Sistema / Mantenimiento) + próxima acción recomendada |
| Estados | texto "…" | `ScanProgress`, `EmptyState`, `ErrorAlert`, toasts |
| Confirmación | `Modal` genérico | `ConfirmDialog` con resumen de cuántos elementos/bytes y lista de omitidos |
| Resultado | nada | `CleanSummary` + recibo |
| Tema | claro/oscuro | Sistema / Claro / Oscuro (menú de apariencia) |
| A11y | parcial | skip-nav, roles ARIA, foco visible, contraste revisado, RTL |

Equivalencias de implementación (Svelte): `HealthScore.svelte`, `StatCard.svelte` (con tween de `svelte/motion`), `PageHeader.svelte`, `AppShell.svelte`, `Sidebar.svelte` con grupos, `ConfirmDialog.svelte` (renombrar/extender el `Modal` actual), `EmptyState`, `ErrorAlert`, `ScanProgress`, `SelectableList`, `ActionBar`, `svelte-sonner` para toasts. Tailwind 3 se mantiene leyendo `rgb(var(--…) / <alpha-value>)`, que es justo lo que 3.0 ya hace.

---

## 7. Plan por fases

Esfuerzos relativos: **S** ≈ días · **M** ≈ 1-2 semanas · **L** ≈ 3+ semanas. Las fases 0 y 1 son prerrequisito del resto; 2-4 pueden solaparse parcialmente.

### Fase 0 — Cimientos y seguridad (S-M) · bloqueante
1. `git init`, `.gitignore`, README, CHANGELOG, `CLAUDE.md`, CI (cargo fmt/clippy/test + `npm run check`).
2. **Rediseñar el helper privilegiado** (S1, S2): binario con operaciones con nombre y validación en el lado root; sin `sh -c` sobre cadenas del frontend; temporales `0600` con `create_new`; polkit con mensaje fiel. Mantener un único prompt por lote (`auth_admin_keep`).
3. Corregir F1: `runJob` devuelve `{success, output}`; solo registrar tras éxito; toast de error con salida.
4. Mover `dir_size_bytes` a `utils`, extraer `SelectableList` + `ActionBar`, `try/catch` en `i18n`.
5. Tests de parsers (`flatpak`, `snap`, `journalctl`, `df`, `apt-mark`) y de validadores de rutas/paquetes (incluyendo `/tmp/x$(id)` y `/tmp/../etc`).
6. CSP estricto y capabilities mínimas; corregir descripción de `capabilities/default.json`.

### Fase 1 — Sistema de diseño Kudu (M)
0. **Plantilla de página rica (§1.1), primer entregable de la fase:** crear los componentes de la plantilla (cabecera compacta, `StatCard` fila, panel de análisis, lista rica, panel de contexto, actividad por categoría, `ActionBar`), eliminar `StatHero` y el banner de foto, y rehacer **todas** las pestañas actuales con ella, una por una, con capturas antes/después.
1. Reescribir `global.css` con la capa final de tokens de Kudu (+ acento Soplos) y mapear `tailwind.config.js`.
2. Crear `AppShell`, `Sidebar` agrupada con submenús, `PageHeader` con pasos, `StatCard`, `HealthScore`, `ConfirmDialog`, `EmptyState`, `ErrorAlert`, `ScanProgress`, toasts.
3. Menú de apariencia **Sistema/Claro/Oscuro**: reutilizar la detección por escritorio de 2.x (`core/environment.py`) en `environment.rs`.
4. Rehacer el Dashboard con anillo de salud, objetivos y "próxima acción" (sin factores inventados: mantener el cálculo honesto actual y documentarlo).
5. Revisión a11y (foco, ARIA, contraste en ambos temas) y prueba en GNOME, Plasma, Xfce, X11 y Wayland.

**Criterio de salida:** las vistas actuales funcionan igual pero con la apariencia y navegación Kudu; sin regresión funcional.

### Fase 2 — Limpiador unificado con reglas (L)
1. Motor en Rust que lea reglas JSON (formato compatible con `rules.schema.json` de Kudu): expansión de `${HOME}`, `${CACHE}`, `${TMPDIR}`, `childSubdir`, `needsAdmin`, `cleanupAction` de lista cerrada.
2. Importar y revisar las reglas Linux de Kudu (`system`, `browsers`, `apps`, `gpu-cache`, `steam`, `gaming`, `databases`) con aviso MIT; añadir reglas específicas de Soplos (Calamares, soplos-welcome, WebApp Manager, AppImage Manager…).
3. Página **Limpiador**: escaneo global → categorías → resultados agrupados con buscar/ordenar/seleccionados → comprobación de procesos bloqueantes → confirmación → **recibo** (borrado/omitido/fallido) → Historial detallado.
4. Exclusiones persistentes. Acciones nativas (`journal-vacuum`, `apt-clean`, `flatpak-unused`, `snap-old`) como operaciones no seleccionadas por defecto con "ahorro variable".
5. Rehacer *Limpieza rápida/completa* como atajos del mismo flujo (siempre con revisión; opción "no volver a preguntar" explícita).
6. Migrar Papelera, Caché usuario, Temp, APT, Logs, Flatpak, Snap a este flujo conservando sus vistas especializadas donde la selección fina aporta (Flatpak, Snap, Apps).

### Fase 3 — Herramientas Soplos de 2.x, de menor a mayor riesgo (L)
Cada módulo: escáner Rust + operación privilegiada con nombre + vista + **tests con las regresiones del CHANGELOG de 2.x** (§5) + modo "simulación".

| Orden | Módulo | Riesgo | Notas |
|---|---|---|---|
| 1 | APT: .deb individuales, listas, `autoremove` con simulación | Bajo | `apt-get -s` para la vista previa |
| 2 | Logs: `/var/log` por entrada y journald configurable (días/tamaño) | Bajo | permitir 7/30 días o tamaño |
| 3 | Idiomas y documentación | Medio | proteger locale activo + inglés; `localepurge` opcional |
| 4 | Resumen del sistema + monitor de recursos | Bajo | variante Soplos, kernel, sesión, uptime, CPU/RAM/temp/GPU |
| 5 | **Kernels** | Alto | proteger en uso + el anterior + kernels Soplos; dirs huérfanos; `update-grub` |
| 6 | Apps instaladas v2: purge + autoremove + initrd cuando proceda | Medio-Alto | ver S3 |
| 7 | **Drivers** por hardware | Alto | detección tres capas, VM guest, protección GPU/red |
| 8 | **Firmware** por familias | Alto | conjunto de protección, propietarios dpkg, marca "temporal" |
| 9 | DKMS huérfanos, referencias de módulos, `dracut.conf.d` | Alto | `dracut -f` serializado; exclusiones duras |
| 10 | Reconstrucción de initrd / bootloader | Alto | solo como paso encadenado y visible |

Regla de oro para 5-10: **nunca borrar sin simulación visible, nunca en el mismo lote que otra operación de initrd, y siempre con copia del estado (lista de paquetes eliminados) en el recibo** para poder reinstalar.

### Fase 4 — Funciones Kudu valiosas en Linux (M-L, por orden de valor)
1. **Historial con recibos** (si no se hizo en la Fase 2) y almacenamiento (tendencia de uso de disco).
2. **Analizador de disco**, archivos grandes, duplicados, carpetas vacías.
3. **Inicio** (XDG autostart + servicios `systemd --user`) y **Servicios** (lectura primero, edición con confirmación).
4. **Cuidado automático**: temporizador `systemd --user` (en lugar de daemon propio) + icono de bandeja opcional.
5. **CLI**: `soplos-sys-cleaner --cli [--clean] [--json] [categorías]` con la semántica de Kudu (por defecto solo escanea).
6. **Onboarding** de 3 pasos y página de ajustes (tema, idioma, exclusiones, retención de historial).
7. Desinstalador con restos (apt + Flatpak + Snap + AppImage) y shredder (opcional).

### Fase 5 — Calidad, empaquetado y salida (M)
- Paridad de claves i18n en CI (8 idiomas, fallar si falta alguna); ampliar idiomas si interesa.
- `Depends/Recommends` correctos, `Replaces/Conflicts/Breaks: soplos-sys-cleaner (<< 3.0)`, desinstalación de la política polkit antigua, AppStream con capturas nuevas.
- Pruebas en VM (VMware/VirtualBox/KVM/Hyper-V) y hardware real con NVIDIA/AMD/Intel, escritorios GNOME/Plasma/Xfce.
- Beta pública con *dry-run* por defecto en módulos de Fase 3; solo habilitar borrado real tras N semanas sin incidencias.
- Retirada de 2.x: congelar en mantenimiento, publicar guía de migración.

---

## 8. Decisiones abiertas (necesito tu criterio)

1. **Paleta**: ¿acento naranja Soplos sobre neutros (propuesto) o el verde oscuro + ámbar de Kudu tal cual? Afecta a todos los tokens.
2. **Titlebar**: ¿decoración nativa (más seguro en Wayland/GNOME/Plasma/Xfce) o titlebar propia como Kudu?
3. **Alcance de Kudu**: ¿solo limpieza + disco + inicio/servicios + automatización (propuesto) o también privacidad/seguridad?
4. **Privilegios**: ¿operaciones con nombre en el mismo binario (propuesto) o helper separado instalado en `/usr/libexec`?
5. **Reglas**: ¿reutilizar y mantener las reglas Linux de Kudu (MIT, con sincronización periódica) o forkearlas a un repositorio propio de Soplos?
6. **Convivencia con 2.x**: ¿mismo nombre de paquete reemplazando a 2.x (propuesto) o paquete nuevo en paralelo hasta paridad?
7. **Distros objetivo**: ¿solo Soplos (Boro/Tyron/Tyson, Debian) o también Ubuntu/Fedora/Arch? Las reglas de Kudu cubren `dnf`/`pacman` y 3.0 asume `apt`.

---

## 9. Riesgos

| Riesgo | Impacto | Mitigación |
|---|---|---|
| Borrar kernel/firmware/driver equivocado deja el sistema sin arrancar | Crítico | simulación obligatoria, proteger kernel en uso + anterior, tests con las regresiones de 2.x, recibo con lista de paquetes |
| Portar de golpe todo Kudu | Alto | recortar alcance (§3.3), fases con criterio de salida |
| Titlebar propia en Wayland/GNOME | Medio | empezar con decoración nativa |
| Estadísticas falsas (F1) minan la confianza | Medio | Fase 0, punto 3 |
| WebKitGTK sin aceleración en VM (blur, animaciones) | Medio | ya hay workaround DMABUF; no usar `backdrop-filter`, limitar animaciones (el `Modal` actual ya lo evita: mantener esa disciplina al portar el estilo "glass") |
| Cambio de modelo de privilegios respecto a 2.x (sin modo Admin persistente) | Medio | prompt único por lote con `auth_admin_keep` y escaneos de solo lectura sin root |

---

## 10. Limitaciones de esta auditoría

- Revisión estática: no se compiló, no se ejecutaron tests ni la app, y no se verificó el comportamiento de `pkexec`/polkit en un sistema real. Los puntos S1, S2 y F1 se deducen directamente del código citado; F4 (Flatpak ámbito usuario) requiere confirmación en un equipo con apps de usuario.
- Kudu se evaluó por estructura, tokens, reglas Linux, CLI y documentación; no se auditó su seguridad ni sus ~1.400 ficheros línea a línea.
- Se leyó el código completo de los módulos Rust y vistas Svelte de 3.0 citados y los scanners/ root helper/ changelog de 2.x; los scanners de hardware (≈1.000 líneas) se revisaron por estructura y changelog, no función a función.
