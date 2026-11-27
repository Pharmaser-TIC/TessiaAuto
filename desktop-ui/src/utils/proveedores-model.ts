// Pure view-model of the third-party card (GET /api/proveedores): which providers deserve a line
// and how to word it. The card shows nothing while everything is fine.

import type { EnEspera, Proveedor, ProveedoresResponse } from "../types/api";
import { fmtTime } from "./format";
import { phaseName } from "./phase-progress";

export interface ProviderState {
  color: "error" | "warning" | "neutral";
  label: string;
}

/** A provider is worth a line when its breaker is not closed or it has recent failures. */
export function needsAttention(p: Proveedor): boolean {
  return p.estado !== "cerrado" || p.fallas > 0;
}

export function providerRows(r: ProveedoresResponse | null | undefined): Proveedor[] {
  return (r?.proveedores ?? []).filter(needsAttention);
}

/** True when there is anything to show: a provider in trouble or invoices waiting for a retry. */
export function hasAttention(r: ProveedoresResponse | null | undefined): boolean {
  return providerRows(r).length > 0 || (r?.en_espera ?? []).length > 0;
}

export function providerState(p: Proveedor): ProviderState {
  if (p.estado === "abierto") return { color: "error", label: "en pausa" };
  if (p.estado === "semiabierto") return { color: "warning", label: "probando" };
  if (p.fallas > 0) return { color: "warning", label: "con fallas" };
  return { color: "neutral", label: "" };
}

/** Secondary line: failures, when it is probed again and the last error; "" when nothing is known. */
export function providerDetail(p: Proveedor): string {
  const parts: string[] = [];
  if (p.fallas > 0) parts.push(`${p.fallas} de ${p.llamadas} llamadas fallaron`);
  if (p.reintento_en_s && p.reintento_en_s > 0) parts.push(`se vuelve a probar en ${p.reintento_en_s} s`);
  if (p.ultimo_error) parts.push(p.ultimo_error);
  return parts.join(" · ");
}

/** "Generación CUV (Ministerio): 2 facturas esperan un reintento · próximo a las 15:00". */
export function waitingText(w: EnEspera, clock: (iso: string) => string = shortTime): string {
  const noun = w.facturas === 1 ? "factura espera" : "facturas esperan";
  const next = clock(w.proximo_reintento);
  return `${phaseName({ fase: w.fase })}: ${w.facturas} ${noun} un reintento${next ? ` · próximo a las ${next}` : ""}`;
}

/** HH:mm in es-CO, from fmtTime's "d/m/yyyy, HH:mm:ss"; "" when the date is unreadable. */
function shortTime(iso: string): string {
  const full = fmtTime(iso);
  const match = /(\d{1,2}:\d{2}):\d{2}/.exec(full);
  return match?.[1] ?? "";
}
