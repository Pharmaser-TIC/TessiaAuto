import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import "../../src/components/proveedores-estado";
import { mountEl, qa, settle, text } from "../support/dashboard-dom";

const holder = vi.hoisted(() => ({ api: {} as Record<string, unknown> }));
vi.mock("../../src/api", () => ({ api: new Proxy({}, { get: (_t, k) => holder.api[k as string] }) }));

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  vi.useRealTimers();
  document.body.innerHTML = "";
});

async function mount(proveedores: () => Promise<unknown>) {
  holder.api = { proveedores: vi.fn(proveedores) };
  const el = await mountEl("tx-proveedores-estado");
  await vi.advanceTimersByTimeAsync(0);
  await settle(el);
  return el;
}

const calm = { proveedores: [{ nombre: "sispro", etiqueta: "SISPRO (Ministerio)", estado: "cerrado", llamadas: 3, fallas: 0, aperturas: 0 }], en_espera: [] };

describe("<tx-proveedores-estado>", () => {
  it("shows nothing while every provider is fine and nothing waits", async () => {
    const el = await mount(async () => calm);
    expect(qa(el, "[data-testid=proveedor]")).toHaveLength(0);
    expect(qa(el, "[data-testid=en-espera]")).toHaveLength(0);
    expect(text(el)).toBe("");
  });

  it("shows a provider that is paused, why, and the invoices waiting for a retry", async () => {
    const el = await mount(async () => ({
      proveedores: [
        { nombre: "cassandra", etiqueta: "Almacenamiento (Cassandra)", estado: "cerrado", llamadas: 10, fallas: 0, aperturas: 0 },
        {
          nombre: "sispro",
          etiqueta: "SISPRO (Ministerio)",
          estado: "abierto",
          llamadas: 8,
          fallas: 8,
          ultimo_error: "sispro: no se pudo contactar el login",
          reintento_en_s: 25,
          aperturas: 1,
        },
      ],
      en_espera: [{ fase: "cuv", facturas: 2, proximo_reintento: "2026-11-27T15:00:00Z" }],
    }));
    const providers = qa(el, "[data-testid=proveedor]");
    expect(providers).toHaveLength(1);
    expect(providers[0]!.getAttribute("data-state")).toBe("abierto");
    expect(providers[0]!.textContent).toContain("SISPRO (Ministerio)");
    expect(providers[0]!.textContent).toContain("en pausa");
    expect(providers[0]!.textContent).toContain("se vuelve a probar en 25 s");
    const waiting = qa(el, "[data-testid=en-espera]");
    expect(waiting).toHaveLength(1);
    expect(waiting[0]!.textContent).toContain("Generación CUV (Ministerio): 2 facturas esperan un reintento");
  });

  it("stays quiet when the request fails: it is an aid, not the health check", async () => {
    const el = await mount(async () => {
      throw new Error("sin ruta");
    });
    expect(text(el)).toBe("");
  });
});
