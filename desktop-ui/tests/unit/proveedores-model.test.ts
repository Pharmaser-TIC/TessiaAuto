import { describe, expect, it } from "vitest";
import type { Proveedor, ProveedoresResponse } from "../../src/types/api";
import {
  hasAttention,
  providerDetail,
  providerRows,
  providerState,
  waitingText,
} from "../../src/utils/proveedores-model";

const base = (over: Partial<Proveedor>): Proveedor => ({
  nombre: "sispro",
  etiqueta: "SISPRO (Ministerio)",
  estado: "cerrado",
  llamadas: 0,
  fallas: 0,
  aperturas: 0,
  ...over,
});

describe("providerState", () => {
  it("is error while open, warning while probing and neutral otherwise", () => {
    expect(providerState(base({ estado: "abierto" }))).toEqual({ color: "error", label: "en pausa" });
    expect(providerState(base({ estado: "semiabierto" }))).toEqual({ color: "warning", label: "probando" });
    expect(providerState(base({ estado: "cerrado", fallas: 2 }))).toEqual({ color: "warning", label: "con fallas" });
    expect(providerState(base({}))).toEqual({ color: "neutral", label: "" });
  });
});

describe("providerRows and hasAttention", () => {
  const answer = (proveedores: Proveedor[], en_espera: ProveedoresResponse["en_espera"] = []): ProveedoresResponse => ({
    proveedores,
    en_espera,
  });

  it("lists only the providers that are not fine", () => {
    const rows = providerRows(answer([base({ nombre: "a" }), base({ nombre: "b", estado: "abierto" }), base({ nombre: "c", fallas: 1 })]));
    expect(rows.map((p) => p.nombre)).toEqual(["b", "c"]);
  });

  it("has nothing to say when every provider is closed without failures and nothing waits", () => {
    expect(hasAttention(answer([base({})]))).toBe(false);
    expect(hasAttention(null)).toBe(false);
    expect(hasAttention(undefined)).toBe(false);
  });

  it("has something to say for a provider in trouble or for invoices waiting", () => {
    expect(hasAttention(answer([base({ estado: "abierto" })]))).toBe(true);
    expect(hasAttention(answer([base({})], [{ fase: "cuv", facturas: 2, proximo_reintento: "2026-11-27T15:00:00Z" }]))).toBe(true);
  });
});

describe("providerDetail", () => {
  it("says how many calls failed, when it is probed again and the last error", () => {
    const text = providerDetail(
      base({ estado: "abierto", llamadas: 8, fallas: 8, reintento_en_s: 25, ultimo_error: "sispro: no se pudo contactar el login" }),
    );
    expect(text).toBe("8 de 8 llamadas fallaron · se vuelve a probar en 25 s · sispro: no se pudo contactar el login");
  });

  it("leaves out what it does not know", () => {
    expect(providerDetail(base({ estado: "semiabierto" }))).toBe("");
    expect(providerDetail(base({ fallas: 2, llamadas: 5 }))).toBe("2 de 5 llamadas fallaron");
  });
});

describe("waitingText", () => {
  it("names the phase, how many invoices wait and the next retry", () => {
    const text = waitingText({ fase: "cuv", facturas: 2, proximo_reintento: "2026-11-27T15:00:00Z" }, () => "15:00");
    expect(text).toBe("Generación CUV (Ministerio): 2 facturas esperan un reintento · próximo a las 15:00");
  });

  it("uses the singular for one invoice", () => {
    const text = waitingText({ fase: "thefactory", facturas: 1, proximo_reintento: "x" }, () => "");
    expect(text).toBe("Descarga de Factura TF: 1 factura espera un reintento");
  });
});
