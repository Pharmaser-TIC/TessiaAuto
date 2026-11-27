import { LitElement, css, html, nothing } from "lit";
import { api } from "../api";
import { PollController } from "../controllers/poll-controller";
import type { ProveedoresResponse } from "../types/api";
import { hasAttention, providerDetail, providerRows, providerState, waitingText } from "../utils/proveedores-model";
import "./tx-badge";
import "./tx-card";

/**
 * Third parties that are paused or failing, and the invoices waiting for a retry because of them
 * (GET /api/proveedores, polled every 5 s). It renders nothing while everything is fine, and stays
 * quiet when the request fails: it is an aid next to the health cards, not a health check.
 */
export class TxProveedoresEstado extends LitElement {
  static styles = css`
    :host {
      display: block;
      padding: var(--tx-space-3) var(--tx-space-4) 0;
    }
    tx-card {
      border-left: 4px solid var(--tx-warning);
    }
    tx-card[data-state="abierto"] {
      border-left-color: var(--tx-error);
    }
    .title {
      font-weight: 700;
      color: var(--tx-text-highlighted);
      margin-bottom: var(--tx-space-2);
    }
    .item + .item {
      margin-top: var(--tx-space-2);
    }
    .head {
      display: flex;
      align-items: center;
      gap: var(--tx-space-2);
      font-weight: 600;
    }
    .detail {
      margin-top: 0.125rem;
      font-size: 0.75rem;
      overflow-wrap: break-word;
      color: var(--tx-text-muted);
    }
    .waiting {
      font-size: 0.8125rem;
      color: var(--tx-text-muted);
    }
  `;

  private poll = new PollController<ProveedoresResponse>(this, () => api.proveedores(), 5000, { pauseWhenHidden: true });

  render() {
    const data = this.poll.data;
    if (!data || !hasAttention(data)) return nothing;
    const rows = providerRows(data);
    const worst = rows.some((p) => p.estado === "abierto") ? "abierto" : "otro";
    return html`<tx-card padding="sm" data-state=${worst}>
      <div class="title">Terceros</div>
      ${rows.map((p) => {
        const state = providerState(p);
        const detail = providerDetail(p);
        return html`<div class="item" data-testid="proveedor" data-state=${p.estado}>
          <div class="head">
            <span>${p.etiqueta}</span>
            ${state.label ? html`<tx-badge size="sm" variant="subtle" color=${state.color}>${state.label}</tx-badge>` : nothing}
          </div>
          ${detail ? html`<div class="detail">${detail}</div>` : nothing}
        </div>`;
      })}
      ${data.en_espera.map((w) => html`<div class="item waiting" data-testid="en-espera">${waitingText(w)}</div>`)}
    </tx-card>`;
  }
}

if (!customElements.get("tx-proveedores-estado")) customElements.define("tx-proveedores-estado", TxProveedoresEstado);
