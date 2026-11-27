# Tarea pendiente: resiliencia ante la caída de terceros

Estado: **en curso** (rama `feat/resiliencia-terceros`). Hechos los pasos 1 y 2; el resto sigue pendiente. Detalle en `odd/tasks/resiliencia-terceros.md`.

## Problema

Un fallo de red o de un tercero (TheFactory, CPScan, Mutual Ser, SISPRO, Cassandra) en una sola factura cancela toda la corrida de `proceso run`. Los terceros siempre van a fallar a veces; hoy esa caída cuesta la corrida entera y debería costar solo un retraso.

Evidencia (corridas del 2026-11-24):
- `derechos validar`: `error sending request` al token de Mutual Ser salió rotulado como "credenciales o token rechazados" (`client.rs:214` convierte todo `AuthError::Token` en `ValidatorError::Authentication`, que es fatal).
- `thefactory descargar`: `request or response body error` al subir a Cassandra canceló la corrida porque `continue_on_error=false` y el error no es reanudable (`pipeline/mod.rs:463`). Reintento de subida: 3 intentos con espera 250 ms (menos de 1 s en total).
- `proceso_estado.next_attempt_at` existe pero ninguna fase lo usa (siempre `None`). Único cortacircuitos: MIPRES.

## Alcance propuesto (en orden)

1. **[hecho]** **Clasificar errores**: transitorio (red, timeout, 5xx, tercero caído) vs definitivo (dato inválido, no encontrado). Separar en `TokenError` el rechazo real (400/401) del fallo de transporte; mostrar la causa real de `reqwest`, no solo "error sending request".
2. **[hecho]** **Transitorios reanudables**: la factura queda para reintentar y la corrida no se cancela. Más margen de reintento en almacenamiento (más intentos, esperas de segundos).
3. **Reintento programado** con `next_attempt_at` (minutos, luego horas).
4. **Cortacircuitos por tercero**: pausar solo la fase afectada cuando su proveedor falla por encima de un umbral; las demás siguen.
5. **Idempotencia en efectos externos**: `cuv` (envío a SISPRO) no se reintenta hoy porque no es idempotente.
6. **Visibilidad por tercero**: tasa de error, trabajo en espera y próximo reintento por proveedor.

Relacionado (más grande, después): latido de claims, cola durable, limitador compartido, worker por fase con el mismo binario. Ver `docs/mapa-de-procesos/00-indice-y-vision.md`.

## Pendiente de validar

- Consulta a Postgres para agrupar los motivos de error de `Convenio_18112026_23112026` (cuántos son de red y cuántos `NO_DISPONIBLE` de TheFactory):
  `SELECT fase, left(motivo,100), count(*) FROM proceso_estado WHERE cargue_id='Convenio_18112026_23112026' AND estado='FAILED' GROUP BY 1,2 ORDER BY 3 DESC;`
- Confirmar que el bucket `testing` de la URL de Cassandra es el esperado.

## Notas

- Cualquier cambio que toque lógica con datos de salud o despliegue en nube requiere revisión con las áreas de seguridad de la información y jurídica (Ley 1581).
- Entorno destino (nube, producción o no) sigue sin definir.
