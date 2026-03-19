import { useState, useMemo } from "react";
import { ArrowLeft, Wallet, Store, AlertTriangle, Clock, CheckCircle, XCircle, ToggleLeft, ToggleRight, Flag, Shield, Zap, ChevronRight } from "lucide-react";

// ─── Mock data ───────────────────────────────────────────────────────────────

const NOW = Math.floor(Date.now() / 1000);
const DAY = 86400;
const WEEK = 7 * DAY;
const MONTH = 30 * DAY;
const YEAR = 365 * DAY;

const MOCK_SERVICES = [
  { service_id: 0, merchant: "GMER...C4Q1", name: "Pi Stream", price: 5_0000000, period_secs: MONTH, trial_period_secs: WEEK, is_active: true, created_at: NOW - 90 * DAY, reports: 0 },
  { service_id: 1, merchant: "GMER...C4Q1", name: "Pi Cloud Storage", price: 49_0000000, period_secs: YEAR, trial_period_secs: 0, is_active: true, created_at: NOW - 60 * DAY, reports: 2 },
  { service_id: 2, merchant: "GDEV...R7X2", name: "Pi VPN Pro", price: 3_0000000, period_secs: MONTH, trial_period_secs: 0, is_active: true, created_at: NOW - 30 * DAY, reports: 7 },
  { service_id: 3, merchant: "GDEV...R7X2", name: "Pi Music", price: 2_0000000, period_secs: MONTH, trial_period_secs: 2 * WEEK, is_active: true, created_at: NOW - 15 * DAY, reports: 12 },
];

const MOCK_SUBSCRIPTIONS = [
  { sub_id: 0, subscriber: "GSUB...USER", service_id: 0, price: 5_0000000, period_secs: MONTH, trial_period_secs: WEEK, trial_end_ts: NOW + 3 * DAY, auto_renew: true, service_end_ts: NOW + 3 * DAY, next_charge_ts: NOW + 3 * DAY, created_at: NOW - 4 * DAY },
  { sub_id: 1, subscriber: "GSUB...USER", service_id: 1, price: 49_0000000, period_secs: YEAR, trial_period_secs: 0, trial_end_ts: 0, auto_renew: true, service_end_ts: NOW + 300 * DAY, next_charge_ts: NOW + 300 * DAY, created_at: NOW - 65 * DAY },
  { sub_id: 2, subscriber: "GSUB...USER", service_id: 2, price: 3_0000000, period_secs: MONTH, trial_period_secs: 0, trial_end_ts: 0, auto_renew: false, service_end_ts: NOW - 5 * DAY, next_charge_ts: NOW - 5 * DAY, created_at: NOW - 35 * DAY },
];

const REPORT_THRESHOLD = 10;
const REPORT_WARNING = 3;
const APPROVE_PERIODS = 12;

// ─── Helpers ─────────────────────────────────────────────────────────────────

const fmtToken = (stroops) => (stroops / 1e7).toFixed(2);

const fmtPeriod = (secs) => {
  if (secs >= YEAR - DAY) return "year";
  if (secs >= MONTH - DAY) return "month";
  if (secs >= WEEK - DAY) return "week";
  if (secs >= DAY - 3600) return `${Math.round(secs / DAY)} days`;
  return `${Math.round(secs / 3600)} hours`;
};

const fmtDate = (ts) => {
  if (!ts) return "—";
  return new Date(ts * 1000).toLocaleDateString("en-US", { month: "short", day: "numeric", year: "numeric" });
};

const fmtCountdown = (ts) => {
  const diff = ts - NOW;
  if (diff <= 0) return "expired";
  const d = Math.floor(diff / DAY);
  if (d > 0) return `${d}d remaining`;
  const h = Math.floor(diff / 3600);
  return `${h}h remaining`;
};

const getSubStatus = (sub) => {
  if (sub.trial_period_secs > 0 && NOW < sub.trial_end_ts) return "trial";
  if (NOW < sub.service_end_ts && sub.auto_renew) return "active";
  if (NOW < sub.service_end_ts && !sub.auto_renew) return "expiring";
  return "expired";
};

const statusColors = {
  trial: { bg: "bg-purple-500/20", text: "text-purple-300", label: "Free Trial" },
  active: { bg: "bg-emerald-500/20", text: "text-emerald-300", label: "Active" },
  expiring: { bg: "bg-amber-500/20", text: "text-amber-300", label: "Expiring" },
  expired: { bg: "bg-zinc-500/20", text: "text-zinc-400", label: "Expired" },
};

// ─── Components ──────────────────────────────────────────────────────────────

function StatusBadge({ status }) {
  const s = statusColors[status];
  return <span className={`px-2 py-0.5 rounded-full text-xs font-medium ${s.bg} ${s.text}`}>{s.label}</span>;
}

function ReportBanner({ reports, blocked }) {
  if (reports < REPORT_WARNING) return null;
  return (
    <div className={`flex items-start gap-2 p-3 rounded-lg text-sm ${blocked ? "bg-red-500/15 text-red-300" : "bg-amber-500/15 text-amber-300"}`}>
      <AlertTriangle size={18} className="shrink-0 mt-0.5" />
      <div>
        <p className="font-medium">{blocked ? "Subscription blocked" : "Warning"}</p>
        <p className="text-xs opacity-80 mt-0.5">
          {blocked
            ? `This service has ${reports} user reports and is blocked from new subscriptions.`
            : `This service has ${reports} user reports for failure to deliver. Proceed with caution.`}
        </p>
      </div>
    </div>
  );
}

// ─── Catalog View ────────────────────────────────────────────────────────────

function CatalogView({ services, onSelect }) {
  return (
    <div className="space-y-4">
      <h2 className="text-lg font-semibold text-zinc-100">Discover Services</h2>
      <div className="space-y-3">
        {services.map((svc) => (
          <button key={svc.service_id} onClick={() => onSelect(svc)} className="w-full text-left bg-zinc-800/60 border border-zinc-700/50 rounded-xl p-4 hover:bg-zinc-800 transition-colors">
            <div className="flex items-start justify-between">
              <div className="space-y-1 flex-1">
                <div className="flex items-center gap-2">
                  <span className="font-medium text-zinc-100">{svc.name}</span>
                  {svc.trial_period_secs > 0 && (
                    <span className="px-1.5 py-0.5 rounded text-[10px] font-medium bg-purple-500/20 text-purple-300">
                      {fmtPeriod(svc.trial_period_secs)} free
                    </span>
                  )}
                </div>
                <p className="text-sm text-zinc-400">{svc.merchant}</p>
                <p className="text-sm font-medium text-zinc-200">{fmtToken(svc.price)} XLM / {fmtPeriod(svc.period_secs)}</p>
              </div>
              <div className="flex items-center gap-2">
                {svc.reports >= REPORT_WARNING && (
                  <span className={`flex items-center gap-1 text-xs ${svc.reports >= REPORT_THRESHOLD ? "text-red-400" : "text-amber-400"}`}>
                    <AlertTriangle size={12} /> {svc.reports}
                  </span>
                )}
                <ChevronRight size={16} className="text-zinc-500" />
              </div>
            </div>
          </button>
        ))}
      </div>
    </div>
  );
}

// ─── Subscribe View ──────────────────────────────────────────────────────────

function SubscribeView({ service, onBack, onSubscribe }) {
  const [autoRenew, setAutoRenew] = useState(true);
  const [confirming, setConfirming] = useState(false);
  const blocked = service.reports >= REPORT_THRESHOLD;
  const approveAmount = fmtToken(service.price * APPROVE_PERIODS);

  const handleSubscribe = () => {
    if (confirming) {
      onSubscribe(service.service_id, autoRenew);
      return;
    }
    setConfirming(true);
  };

  return (
    <div className="space-y-4">
      <button onClick={onBack} className="flex items-center gap-1 text-sm text-zinc-400 hover:text-zinc-200 transition-colors">
        <ArrowLeft size={16} /> Back
      </button>

      <div className="bg-zinc-800/60 border border-zinc-700/50 rounded-xl p-5 space-y-4">
        <div>
          <h2 className="text-xl font-semibold text-zinc-100">{service.name}</h2>
          <p className="text-sm text-zinc-400 mt-1">by {service.merchant}</p>
        </div>

        <ReportBanner reports={service.reports} blocked={blocked} />

        {/* Plan details */}
        <div className="space-y-3 text-sm">
          <div className="flex justify-between py-2 border-b border-zinc-700/50">
            <span className="text-zinc-400">Price</span>
            <span className="text-zinc-100 font-medium">{fmtToken(service.price)} XLM / {fmtPeriod(service.period_secs)}</span>
          </div>
          <div className="flex justify-between py-2 border-b border-zinc-700/50">
            <span className="text-zinc-400">Billing cycle</span>
            <span className="text-zinc-100">{fmtPeriod(service.period_secs)}ly</span>
          </div>
          {service.trial_period_secs > 0 && (
            <div className="flex justify-between py-2 border-b border-zinc-700/50">
              <span className="text-zinc-400">Free trial</span>
              <span className="text-purple-300 font-medium">{fmtPeriod(service.trial_period_secs)}</span>
            </div>
          )}
          <div className="flex justify-between py-2 border-b border-zinc-700/50">
            <span className="text-zinc-400">First charge</span>
            <span className="text-zinc-100">
              {service.trial_period_secs > 0
                ? `After ${fmtPeriod(service.trial_period_secs)} trial ends`
                : "Immediately"}
            </span>
          </div>
        </div>

        {/* Auto-renew toggle */}
        <div className="flex items-center justify-between bg-zinc-900/50 rounded-lg p-3">
          <div>
            <p className="text-sm font-medium text-zinc-200">Auto-renew</p>
            <p className="text-xs text-zinc-500 mt-0.5">
              {autoRenew ? `Approves ${approveAmount} XLM for ${APPROVE_PERIODS} periods` : "Single period only, no approval needed"}
            </p>
          </div>
          <button onClick={() => setAutoRenew(!autoRenew)} className="text-zinc-300">
            {autoRenew ? <ToggleRight size={28} className="text-emerald-400" /> : <ToggleLeft size={28} className="text-zinc-500" />}
          </button>
        </div>

        {/* Approval info */}
        {autoRenew && (
          <div className="flex items-start gap-2 p-3 rounded-lg bg-blue-500/10 text-blue-300 text-xs">
            <Shield size={14} className="shrink-0 mt-0.5" />
            <p>
              Your wallet will approve the contract to pull up to {approveAmount} XLM
              over {APPROVE_PERIODS} billing cycles. You can revoke anytime by toggling
              auto-renew off.
            </p>
          </div>
        )}

        {/* Subscribe button */}
        <button
          onClick={handleSubscribe}
          disabled={blocked}
          className={`w-full py-3 rounded-lg font-medium text-sm transition-all ${
            blocked
              ? "bg-zinc-700 text-zinc-500 cursor-not-allowed"
              : confirming
              ? "bg-emerald-600 hover:bg-emerald-500 text-white"
              : "bg-indigo-600 hover:bg-indigo-500 text-white"
          }`}
        >
          {blocked
            ? "Blocked — too many reports"
            : confirming
            ? `Confirm — ${service.trial_period_secs > 0 ? "Start Free Trial" : `Pay ${fmtToken(service.price)} XLM`}`
            : "Subscribe"}
        </button>

        {confirming && !blocked && (
          <p className="text-center text-xs text-zinc-500">
            {service.trial_period_secs > 0
              ? "No charge during trial. Cancel anytime."
              : "You will be charged immediately for the first period."}
          </p>
        )}
      </div>
    </div>
  );
}

// ─── Wallet View ─────────────────────────────────────────────────────────────

function WalletView({ subscriptions, services, onSelect }) {
  const [tab, setTab] = useState("active");

  const enriched = subscriptions.map((sub) => ({
    ...sub,
    status: getSubStatus(sub),
    service: services.find((s) => s.service_id === sub.service_id),
  }));

  const active = enriched.filter((s) => s.status !== "expired");
  const inactive = enriched.filter((s) => s.status === "expired");
  const displayed = tab === "active" ? active : inactive;

  return (
    <div className="space-y-4">
      <h2 className="text-lg font-semibold text-zinc-100">My Subscriptions</h2>

      {/* Tabs */}
      <div className="flex bg-zinc-800/60 rounded-lg p-1">
        {["active", "inactive"].map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            className={`flex-1 py-2 text-sm font-medium rounded-md transition-colors ${
              tab === t ? "bg-zinc-700 text-zinc-100" : "text-zinc-400 hover:text-zinc-300"
            }`}
          >
            {t === "active" ? `Active (${active.length})` : `Inactive (${inactive.length})`}
          </button>
        ))}
      </div>

      {/* Sub cards */}
      {displayed.length === 0 ? (
        <div className="text-center py-10 text-zinc-500 text-sm">
          {tab === "active" ? "No active subscriptions" : "No inactive subscriptions"}
        </div>
      ) : (
        <div className="space-y-3">
          {displayed.map((sub) => (
            <button key={sub.sub_id} onClick={() => onSelect(sub)} className="w-full text-left bg-zinc-800/60 border border-zinc-700/50 rounded-xl p-4 hover:bg-zinc-800 transition-colors">
              <div className="flex items-start justify-between">
                <div className="space-y-1">
                  <div className="flex items-center gap-2">
                    <span className="font-medium text-zinc-100">{sub.service?.name ?? `Service #${sub.service_id}`}</span>
                    <StatusBadge status={sub.status} />
                  </div>
                  <p className="text-sm text-zinc-400">{fmtToken(sub.price)} XLM / {fmtPeriod(sub.period_secs)}</p>
                  {sub.status !== "expired" && (
                    <p className="text-xs text-zinc-500">
                      {sub.status === "trial"
                        ? `Trial ends ${fmtDate(sub.trial_end_ts)} (${fmtCountdown(sub.trial_end_ts)})`
                        : sub.auto_renew
                        ? `Next charge ${fmtDate(sub.next_charge_ts)}`
                        : `Expires ${fmtDate(sub.service_end_ts)} (${fmtCountdown(sub.service_end_ts)})`}
                    </p>
                  )}
                </div>
                <ChevronRight size={16} className="text-zinc-500 mt-1" />
              </div>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

// ─── Manage View ─────────────────────────────────────────────────────────────

function ManageView({ sub: initialSub, service, onBack, onToggleRenew, onCancel, onReport }) {
  const [sub, setSub] = useState(initialSub);
  const [showCancelConfirm, setShowCancelConfirm] = useState(false);
  const [reported, setReported] = useState(false);
  const status = getSubStatus(sub);

  const handleToggle = () => {
    const newVal = !sub.auto_renew;
    setSub({ ...sub, auto_renew: newVal });
    onToggleRenew(sub.sub_id);
  };

  const handleCancel = () => {
    if (!showCancelConfirm) { setShowCancelConfirm(true); return; }
    setSub({ ...sub, auto_renew: false });
    setShowCancelConfirm(false);
    onCancel(sub.sub_id);
  };

  const handleReport = () => {
    setReported(true);
    onReport(sub.service_id);
  };

  const progressPct = (() => {
    const total = sub.trial_period_secs > 0 && NOW < sub.trial_end_ts
      ? sub.trial_period_secs
      : sub.period_secs;
    const endTs = sub.trial_period_secs > 0 && NOW < sub.trial_end_ts
      ? sub.trial_end_ts
      : sub.service_end_ts;
    const startTs = endTs - total;
    const elapsed = NOW - startTs;
    return Math.max(0, Math.min(100, (elapsed / total) * 100));
  })();

  return (
    <div className="space-y-4">
      <button onClick={onBack} className="flex items-center gap-1 text-sm text-zinc-400 hover:text-zinc-200 transition-colors">
        <ArrowLeft size={16} /> My Subscriptions
      </button>

      <div className="bg-zinc-800/60 border border-zinc-700/50 rounded-xl p-5 space-y-5">
        {/* Header */}
        <div className="flex items-start justify-between">
          <div>
            <h2 className="text-xl font-semibold text-zinc-100">{service?.name ?? `Service #${sub.service_id}`}</h2>
            <p className="text-sm text-zinc-400 mt-1">{service?.merchant}</p>
          </div>
          <StatusBadge status={status} />
        </div>

        {/* Progress bar */}
        {status !== "expired" && (
          <div>
            <div className="flex justify-between text-xs text-zinc-500 mb-1">
              <span>Current {status === "trial" ? "trial" : "period"}</span>
              <span>{fmtCountdown(status === "trial" ? sub.trial_end_ts : sub.service_end_ts)}</span>
            </div>
            <div className="h-2 bg-zinc-700 rounded-full overflow-hidden">
              <div
                className={`h-full rounded-full transition-all ${status === "trial" ? "bg-purple-500" : status === "expiring" ? "bg-amber-500" : "bg-emerald-500"}`}
                style={{ width: `${progressPct}%` }}
              />
            </div>
          </div>
        )}

        {/* Details */}
        <div className="space-y-2 text-sm">
          <div className="flex justify-between py-2 border-b border-zinc-700/50">
            <span className="text-zinc-400">Price</span>
            <span className="text-zinc-100">{fmtToken(sub.price)} XLM / {fmtPeriod(sub.period_secs)}</span>
          </div>
          {sub.trial_period_secs > 0 && (
            <div className="flex justify-between py-2 border-b border-zinc-700/50">
              <span className="text-zinc-400">Trial ends</span>
              <span className="text-zinc-100">{fmtDate(sub.trial_end_ts)}</span>
            </div>
          )}
          <div className="flex justify-between py-2 border-b border-zinc-700/50">
            <span className="text-zinc-400">Service active until</span>
            <span className="text-zinc-100">{fmtDate(sub.service_end_ts)}</span>
          </div>
          <div className="flex justify-between py-2 border-b border-zinc-700/50">
            <span className="text-zinc-400">Next charge</span>
            <span className="text-zinc-100">{sub.auto_renew ? fmtDate(sub.next_charge_ts) : "—"}</span>
          </div>
          <div className="flex justify-between py-2 border-b border-zinc-700/50">
            <span className="text-zinc-400">Subscribed</span>
            <span className="text-zinc-100">{fmtDate(sub.created_at)}</span>
          </div>
        </div>

        {/* Auto-renew toggle */}
        {status !== "expired" && (
          <div className="flex items-center justify-between bg-zinc-900/50 rounded-lg p-3">
            <div>
              <p className="text-sm font-medium text-zinc-200">Auto-renew</p>
              <p className="text-xs text-zinc-500 mt-0.5">
                {sub.auto_renew ? "Will renew automatically" : "Will not renew after current period"}
              </p>
            </div>
            <button onClick={handleToggle} className="text-zinc-300">
              {sub.auto_renew ? <ToggleRight size={28} className="text-emerald-400" /> : <ToggleLeft size={28} className="text-zinc-500" />}
            </button>
          </div>
        )}

        {/* Cancel */}
        {sub.auto_renew && status !== "expired" && (
          <button
            onClick={handleCancel}
            className={`w-full py-3 rounded-lg font-medium text-sm transition-all ${
              showCancelConfirm
                ? "bg-red-600 hover:bg-red-500 text-white"
                : "bg-zinc-700 hover:bg-zinc-600 text-zinc-300"
            }`}
          >
            {showCancelConfirm ? "Confirm cancellation — no refunds" : "Cancel subscription"}
          </button>
        )}
        {showCancelConfirm && (
          <p className="text-xs text-zinc-500 text-center">
            Your access continues until {fmtDate(sub.service_end_ts)}. No partial refunds will be issued.
          </p>
        )}

        {/* Report */}
        {status !== "expired" && (
          <div className="pt-2 border-t border-zinc-700/50">
            <button
              onClick={handleReport}
              disabled={reported}
              className={`flex items-center gap-2 text-sm transition-colors ${
                reported ? "text-zinc-600 cursor-not-allowed" : "text-amber-400 hover:text-amber-300"
              }`}
            >
              <Flag size={14} />
              {reported ? "Reported — thank you" : "Report service for failure to deliver"}
            </button>
            {service && service.reports > 0 && (
              <p className="text-xs text-zinc-600 mt-1">{service.reports} total report{service.reports !== 1 ? "s" : ""} on this service</p>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

// ─── Main App ────────────────────────────────────────────────────────────────

export default function SubscriptionApp() {
  const [view, setView] = useState("catalog"); // catalog | subscribe | wallet | manage
  const [selectedService, setSelectedService] = useState(null);
  const [selectedSub, setSelectedSub] = useState(null);
  const [subscriptions, setSubscriptions] = useState(MOCK_SUBSCRIPTIONS);
  const [services] = useState(MOCK_SERVICES);
  const [toast, setToast] = useState(null);

  const showToast = (msg) => {
    setToast(msg);
    setTimeout(() => setToast(null), 3000);
  };

  const handleSelectService = (svc) => {
    setSelectedService(svc);
    setView("subscribe");
  };

  const handleSubscribe = (serviceId, autoRenew) => {
    const svc = services.find((s) => s.service_id === serviceId);
    const newSub = {
      sub_id: subscriptions.length,
      subscriber: "GSUB...USER",
      service_id: serviceId,
      price: svc.price,
      period_secs: svc.period_secs,
      trial_period_secs: svc.trial_period_secs,
      trial_end_ts: svc.trial_period_secs > 0 ? NOW + svc.trial_period_secs : 0,
      auto_renew: autoRenew,
      service_end_ts: svc.trial_period_secs > 0 ? NOW + svc.trial_period_secs : NOW + svc.period_secs,
      next_charge_ts: svc.trial_period_secs > 0 ? NOW + svc.trial_period_secs : NOW + svc.period_secs,
      created_at: NOW,
    };
    setSubscriptions((prev) => [...prev, newSub]);
    showToast(svc.trial_period_secs > 0 ? `Trial started for ${svc.name}` : `Subscribed to ${svc.name}`);
    setView("wallet");
  };

  const handleSelectSub = (sub) => {
    setSelectedSub(sub);
    setView("manage");
  };

  const handleToggleRenew = (subId) => {
    setSubscriptions((prev) => prev.map((s) => s.sub_id === subId ? { ...s, auto_renew: !s.auto_renew } : s));
    showToast("Auto-renew updated");
  };

  const handleCancel = (subId) => {
    setSubscriptions((prev) => prev.map((s) => s.sub_id === subId ? { ...s, auto_renew: false } : s));
    showToast("Subscription cancelled");
  };

  const handleReport = (serviceId) => {
    showToast("Report submitted");
  };

  // Bottom nav
  const navItems = [
    { id: "catalog", icon: Store, label: "Services" },
    { id: "wallet", icon: Wallet, label: "Wallet" },
  ];

  return (
    <div className="min-h-screen bg-zinc-900 text-zinc-100 flex flex-col" style={{ maxWidth: 480, margin: "0 auto" }}>
      {/* Toast */}
      {toast && (
        <div className="fixed top-4 left-1/2 -translate-x-1/2 z-50 bg-emerald-600 text-white text-sm px-4 py-2 rounded-lg shadow-lg animate-pulse">
          {toast}
        </div>
      )}

      {/* Content */}
      <div className="flex-1 px-4 pt-6 pb-20 overflow-y-auto">
        {view === "catalog" && <CatalogView services={services} onSelect={handleSelectService} />}
        {view === "subscribe" && selectedService && (
          <SubscribeView
            service={selectedService}
            onBack={() => setView("catalog")}
            onSubscribe={handleSubscribe}
          />
        )}
        {view === "wallet" && (
          <WalletView
            subscriptions={subscriptions}
            services={services}
            onSelect={handleSelectSub}
          />
        )}
        {view === "manage" && selectedSub && (
          <ManageView
            sub={selectedSub}
            service={services.find((s) => s.service_id === selectedSub.service_id)}
            onBack={() => setView("wallet")}
            onToggleRenew={handleToggleRenew}
            onCancel={handleCancel}
            onReport={handleReport}
          />
        )}
      </div>

      {/* Bottom nav */}
      <nav className="fixed bottom-0 left-1/2 -translate-x-1/2 w-full bg-zinc-900/95 border-t border-zinc-800 backdrop-blur-sm" style={{ maxWidth: 480 }}>
        <div className="flex">
          {navItems.map(({ id, icon: Icon, label }) => (
            <button
              key={id}
              onClick={() => setView(id)}
              className={`flex-1 flex flex-col items-center py-3 text-xs transition-colors ${
                (view === id || (id === "catalog" && view === "subscribe") || (id === "wallet" && view === "manage"))
                  ? "text-indigo-400"
                  : "text-zinc-500 hover:text-zinc-300"
              }`}
            >
              <Icon size={20} />
              <span className="mt-1">{label}</span>
            </button>
          ))}
        </div>
      </nav>
    </div>
  );
}
