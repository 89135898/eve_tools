import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  getAuthStatus,
  getBackendConnectionStatus,
  getSyncStatus,
  listOrderMonitorItems,
  listSelectionCandidates,
  listTradeHubs,
  lookupMarketPrice,
  startEveSsoLogin,
  syncCharacterOrders,
  type AuthStatusView,
  type BackendConnectionStatusView,
  type MarketLookupView,
  type OrderMonitorView,
  type SelectionCandidateView,
  type SyncStatus,
  type TradeHubView
} from "./commands";
import { supportedLanguages, translateCode, type SupportedLanguage } from "./i18n";

type LoadState = "idle" | "loading" | "ready" | "error";
type RefreshResult = {
  lookup: MarketLookupView;
  candidates: SelectionCandidateView[];
  hubs: TradeHubView[];
  orders: OrderMonitorView[];
  syncStatus: SyncStatus;
  authStatus: AuthStatusView;
};

let refreshRequestInFlight: Promise<RefreshResult> | null = null;

async function runRefreshRequest(query: string, language: string, selectedHubId: string): Promise<RefreshResult> {
  const lookupResult = await lookupMarketPrice(query, language);
  const hubResult = await listTradeHubs();
  const hubIds = selectedHubId === "all" ? [] : [selectedHubId];
  const candidateResult = await listSelectionCandidates(language, hubIds);
  const candidateStatus = await getSyncStatus();
  const authStatus = await getAuthStatus();
  const orderResult = await listOrderMonitorItems(language);
  return {
    lookup: lookupResult,
    candidates: candidateResult,
    hubs: hubResult,
    orders: orderResult,
    syncStatus: candidateStatus,
    authStatus
  };
}

async function safeGetBackendConnectionStatus(): Promise<BackendConnectionStatusView | null> {
  try {
    return await getBackendConnectionStatus();
  } catch {
    return null;
  }
}

function runSingleFlightRefresh(query: string, language: string, selectedHubId: string): Promise<RefreshResult> {
  if (refreshRequestInFlight) {
    return refreshRequestInFlight;
  }
  refreshRequestInFlight = runRefreshRequest(query, language, selectedHubId).finally(() => {
    refreshRequestInFlight = null;
  });
  return refreshRequestInFlight;
}

export default function App() {
  const { i18n, t } = useTranslation();
  const [query, setQuery] = useState("Tritanium");
  const [lookup, setLookup] = useState<MarketLookupView | null>(null);
  const [candidates, setCandidates] = useState<SelectionCandidateView[]>([]);
  const [hubs, setHubs] = useState<TradeHubView[]>([]);
  const [selectedHubId, setSelectedHubId] = useState("all");
  const [orders, setOrders] = useState<OrderMonitorView[]>([]);
  const [syncStatus, setSyncStatus] = useState<SyncStatus | null>(null);
  const [authStatus, setAuthStatus] = useState<AuthStatusView | null>(null);
  const [backendStatus, setBackendStatus] = useState<BackendConnectionStatusView | null>(null);
  const [loadState, setLoadState] = useState<LoadState>("idle");
  const [authBusy, setAuthBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const refreshInFlight = useRef(false);
  const mountedRef = useRef(true);
  const language = i18n.resolvedLanguage as SupportedLanguage;
  const numberFormatter = new Intl.NumberFormat(language);

  function code(prefix: string, value: string | undefined) {
    return translateCode(prefix, value ?? "unknown", t);
  }

  function formatReasons(reasonCodes: string[]) {
    return reasonCodes.map((reasonCode) => code("codes.reason", reasonCode)).join(", ");
  }

  async function refresh() {
    if (refreshInFlight.current) {
      return;
    }
    refreshInFlight.current = true;
    setLoadState("loading");
    setError(null);
    const latestBackendStatus = await safeGetBackendConnectionStatus();
    if (!mountedRef.current) {
      refreshInFlight.current = false;
      return;
    }
    setBackendStatus(latestBackendStatus);
    try {
      const result = await runSingleFlightRefresh(query, language, selectedHubId);
      if (!mountedRef.current) {
        return;
      }
      setLookup(result.lookup);
      setCandidates(result.candidates);
      setHubs(result.hubs);
      setOrders(result.orders);
      setSyncStatus(result.syncStatus);
      setAuthStatus(result.authStatus);
      setLoadState("ready");
    } catch (err) {
      if (!mountedRef.current) {
        return;
      }
      setLookup(null);
      setCandidates([]);
      setHubs([]);
      setOrders([]);
      setSyncStatus(null);
      setAuthStatus(null);
      setError(err instanceof Error ? err.message : String(err));
      setLoadState("error");
    } finally {
      refreshInFlight.current = false;
    }
  }

  useEffect(() => {
    mountedRef.current = true;
    void refresh();
    return () => {
      mountedRef.current = false;
    };
  }, [language, selectedHubId]);

  async function runAuthAction(action: () => Promise<unknown>) {
    if (authBusy) {
      return;
    }
    setAuthBusy(true);
    setError(null);
    try {
      await action();
      await refresh();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setAuthBusy(false);
    }
  }

  return (
    <main className="app-shell">
      <header className="topbar">
        <div>
          <h1>{t("app.title")}</h1>
          <p>{t("app.subtitle")}</p>
        </div>
        <div className="topbar-actions">
          <label className="language-select">
            <span>{t("language.label")}</span>
            <select
              value={language}
              onChange={(event) => void i18n.changeLanguage(event.target.value)}
              aria-label={t("language.label")}
            >
              {supportedLanguages.map((option) => (
                <option key={option.code} value={option.code}>
                  {t(option.labelKey)}
                </option>
              ))}
            </select>
          </label>
          <button type="button" onClick={() => void refresh()} disabled={loadState === "loading"}>
            {loadState === "loading" ? t("actions.refreshing") : t("actions.refresh")}
          </button>
        </div>
      </header>

      <section className="status-row">
        <StatusCard
          label={t("statusCards.backendApi")}
          value={code("codes.backendStatus", backendStatus?.overall_status)}
          detail={backendStatus?.base_url ?? undefined}
        />
        <StatusCard label={t("statusCards.publicMarketSync")} value={code("codes.syncStatus", syncStatus?.public_market_sync)} />
        <StatusCard
          label={t("statusCards.orderSync")}
          value={code("codes.syncStatus", authStatus?.status ?? syncStatus?.authenticated_order_sync)}
          detail={authStatus?.character_name ?? undefined}
        />
        <StatusCard label={t("statusCards.dataSource")} value={code("codes.dataSource", syncStatus?.data_source)} />
      </section>

      {error && <div className="error-banner">{error}</div>}

      {backendStatus && backendStatus.overall_status !== "ready" && (
        <section className="backend-probes" aria-label={t("statusCards.backendApi")}>
          {backendStatus.probes.map((probe) => (
            <span key={probe.path}>
              {probe.path}: {code("codes.backendProbe", probe.status)}
            </span>
          ))}
        </section>
      )}

      <section className="panel lookup-panel">
        <div className="panel-header">
          <h2>{t("lookup.title")}</h2>
          <form
            onSubmit={(event) => {
              event.preventDefault();
              void refresh();
            }}
          >
            <input value={query} onChange={(event) => setQuery(event.target.value)} aria-label={t("lookup.itemQuery")} />
            <button type="submit" disabled={loadState === "loading"}>
              {t("actions.lookup")}
            </button>
          </form>
        </div>
        {lookup && (
          <div className="metric-grid">
            <Metric label={t("lookup.item")} value={lookup.item_name} />
            <Metric label={t("lookup.bestBid")} value={lookup.best_bid} />
            <Metric label={t("lookup.bestAsk")} value={lookup.best_ask} />
            <Metric label={t("lookup.spread")} value={`${lookup.spread} (${lookup.spread_percent}%)`} />
            <Metric label={t("lookup.dailyVolume")} value={numberFormatter.format(lookup.daily_volume)} />
            <Metric label={t("lookup.dataQuality")} value={code("codes.dataQuality", lookup.data_quality)} />
          </div>
        )}
      </section>

      <section className="dashboard-grid">
        <section className="panel">
          <div className="panel-header">
            <h2>{t("selection.title")}</h2>
            <div className="panel-actions">
              <select
                value={selectedHubId}
                onChange={(event) => setSelectedHubId(event.target.value)}
                aria-label={t("selection.hub")}
              >
                <option value="all">{t("selection.allHubs")}</option>
                {hubs.map((hub) => (
                  <option key={hub.hub_id} value={hub.hub_id}>
                    {hub.display_name}
                  </option>
                ))}
              </select>
              <span>{t("selection.count", { count: candidates.length })}</span>
            </div>
          </div>
          <table>
            <thead>
              <tr>
                <th>{t("selection.hub")}</th>
                <th>{t("selection.item")}</th>
                <th>{t("selection.entry")}</th>
                <th>{t("selection.exit")}</th>
                <th>{t("selection.net")}</th>
                <th>{t("selection.attention")}</th>
                <th>{t("selection.reasons")}</th>
              </tr>
            </thead>
            <tbody>
              {candidates.map((candidate) => (
                <tr key={`${candidate.hub_id}-${candidate.type_id}`}>
                  <td>{candidate.hub_name}</td>
                  <td>{candidate.item_name}</td>
                  <td>{candidate.recommended_entry_price}</td>
                  <td>{candidate.recommended_exit_price}</td>
                  <td>{candidate.net_profit}</td>
                  <td>{numberFormatter.format(candidate.attention_score)}</td>
                  <td>{formatReasons(candidate.reason_codes)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>

        <section className="panel">
          <div className="panel-header">
            <h2>{t("orders.title")}</h2>
            <div className="panel-actions">
              <button
                type="button"
                onClick={() => void runAuthAction(startEveSsoLogin)}
                disabled={authBusy || loadState === "loading"}
              >
                {authBusy ? t("actions.authorizing") : t("actions.login")}
              </button>
              <button
                type="button"
                onClick={() => void runAuthAction(syncCharacterOrders)}
                disabled={authBusy || loadState === "loading" || authStatus?.status !== "authorized"}
              >
                {t("actions.syncOrders")}
              </button>
              <span>{t("orders.count", { count: orders.length })}</span>
            </div>
          </div>
          <table>
            <thead>
              <tr>
                <th>{t("orders.item")}</th>
                <th>{t("orders.side")}</th>
                <th>{t("orders.current")}</th>
                <th>{t("orders.leader")}</th>
                <th>{t("orders.recommended")}</th>
                <th>{t("orders.action")}</th>
                <th>{t("orders.urgency")}</th>
                <th>{t("orders.reasons")}</th>
              </tr>
            </thead>
            <tbody>
              {orders.map((order) => (
                <tr key={order.order_id}>
                  <td>{order.item_name}</td>
                  <td>{code("codes.side", order.side)}</td>
                  <td>{order.current_price}</td>
                  <td>{order.market_leader_price}</td>
                  <td>{order.recommended_price}</td>
                  <td>{code("codes.action", order.recommended_action)}</td>
                  <td>{numberFormatter.format(order.urgency_score)}</td>
                  <td>{formatReasons(order.reason_codes)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      </section>
    </main>
  );
}

function StatusCard({ label, value, detail }: { label: string; value: string; detail?: string }) {
  return (
    <div className="status-card">
      <span>{label}</span>
      <strong>{value}</strong>
      {detail && <small>{detail}</small>}
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
