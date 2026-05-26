import { useEffect, useState } from "react";
import {
  getSyncStatus,
  listOrderMonitorItems,
  listSelectionCandidates,
  lookupMarketPrice,
  type MarketLookupView,
  type OrderMonitorView,
  type SelectionCandidateView,
  type SyncStatus
} from "./commands";

type LoadState = "idle" | "loading" | "ready" | "error";

export default function App() {
  const [query, setQuery] = useState("Tritanium");
  const [lookup, setLookup] = useState<MarketLookupView | null>(null);
  const [candidates, setCandidates] = useState<SelectionCandidateView[]>([]);
  const [orders, setOrders] = useState<OrderMonitorView[]>([]);
  const [syncStatus, setSyncStatus] = useState<SyncStatus | null>(null);
  const [loadState, setLoadState] = useState<LoadState>("idle");
  const [error, setError] = useState<string | null>(null);

  async function refresh() {
    setLoadState("loading");
    setError(null);
    try {
      const [lookupResult, candidateResult, orderResult, statusResult] = await Promise.all([
        lookupMarketPrice(query),
        listSelectionCandidates(),
        listOrderMonitorItems(),
        getSyncStatus()
      ]);
      setLookup(lookupResult);
      setCandidates(candidateResult);
      setOrders(orderResult);
      setSyncStatus(statusResult);
      setLoadState("ready");
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setLoadState("error");
    }
  }

  useEffect(() => {
    void refresh();
  }, []);

  return (
    <main className="app-shell">
      <header className="topbar">
        <div>
          <h1>EVE Trader Assistant</h1>
          <p>Jita 4-4 station trading cockpit</p>
        </div>
        <button type="button" onClick={() => void refresh()} disabled={loadState === "loading"}>
          {loadState === "loading" ? "Refreshing" : "Refresh"}
        </button>
      </header>

      <section className="status-row">
        <StatusCard label="Public market sync" value={syncStatus?.public_market_sync ?? "unknown"} />
        <StatusCard label="Order sync" value={syncStatus?.authenticated_order_sync ?? "unknown"} />
        <StatusCard label="Data source" value="fixture" />
      </section>

      {error && <div className="error-banner">{error}</div>}

      <section className="panel lookup-panel">
        <div className="panel-header">
          <h2>Market Price Lookup</h2>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void refresh();
            }}
          >
            <input value={query} onChange={(event) => setQuery(event.target.value)} aria-label="Item query" />
            <button type="submit">Lookup</button>
          </form>
        </div>
        {lookup && (
          <div className="metric-grid">
            <Metric label="Item" value={lookup.item_name} />
            <Metric label="Best bid" value={lookup.best_bid} />
            <Metric label="Best ask" value={lookup.best_ask} />
            <Metric label="Spread" value={`${lookup.spread} (${lookup.spread_percent}%)`} />
            <Metric label="Daily volume" value={lookup.daily_volume.toLocaleString()} />
            <Metric label="Data quality" value={lookup.data_quality} />
          </div>
        )}
      </section>

      <section className="dashboard-grid">
        <section className="panel">
          <div className="panel-header">
            <h2>Selection Discovery</h2>
            <span>{candidates.length} candidates</span>
          </div>
          <table>
            <thead>
              <tr>
                <th>Item</th>
                <th>Entry</th>
                <th>Exit</th>
                <th>Net</th>
                <th>Attention</th>
                <th>Reasons</th>
              </tr>
            </thead>
            <tbody>
              {candidates.map((candidate) => (
                <tr key={candidate.type_id}>
                  <td>{candidate.item_name}</td>
                  <td>{candidate.recommended_entry_price}</td>
                  <td>{candidate.recommended_exit_price}</td>
                  <td>{candidate.net_profit}</td>
                  <td>{candidate.attention_score}</td>
                  <td>{candidate.reason_codes.join(", ")}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>

        <section className="panel">
          <div className="panel-header">
            <h2>Order Monitor</h2>
            <span>{orders.length} orders</span>
          </div>
          <table>
            <thead>
              <tr>
                <th>Item</th>
                <th>Side</th>
                <th>Current</th>
                <th>Leader</th>
                <th>Recommended</th>
                <th>Urgency</th>
              </tr>
            </thead>
            <tbody>
              {orders.map((order) => (
                <tr key={order.order_id}>
                  <td>{order.item_name}</td>
                  <td>{order.side}</td>
                  <td>{order.current_price}</td>
                  <td>{order.market_leader_price}</td>
                  <td>{order.recommended_price}</td>
                  <td>{order.urgency_score}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      </section>
    </main>
  );
}

function StatusCard({ label, value }: { label: string; value: string }) {
  return (
    <div className="status-card">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="metric">
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}
