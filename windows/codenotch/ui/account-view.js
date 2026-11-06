/* Shared, testable account presentation. Never accepts or renders credential values. */
(function (root) {
  'use strict';
  const escape = value => String(value == null ? '' : value).replace(/[&<>"']/g, c => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
  const providerName = provider => ({claude:'Claude',codex:'Codex',opencode:'OpenCode'})[provider] || provider;
  function cells(records, metadata) {
    const configured = new Map((metadata || []).map(a => [a.id, a]));
    return (records || []).filter(item => {
      const account = configured.get(item.account_id);
      return (!account || account.enabled) && item.snapshot && item.snapshot.status !== 'absent';
    }).map(item => ({id:item.account_id,base:item.provider,
      name:providerName(item.provider)+' · '+(configured.get(item.account_id)?.label || item.label),
      glyph:item.provider==='codex'?'Cx':'OC',snap:item.snapshot}));
  }
  function quotaRows(account, now=Date.now()) {
    return (Array.isArray(account.windows)?account.windows:[]).filter(window=>
      typeof window.used==='number'&&Number.isFinite(window.used)&&window.count==null
    ).map(window=>{
      const used=Math.max(0,Math.min(1,window.used))*100;
      const normalized=String(window.label||'').trim().toLowerCase();
      const labelKey=['5h limit','5-hour limit','5h rolling'].includes(normalized)?'quota_5h':
        ['weekly','weekly limit'].includes(normalized)?'quota_weekly':
        ['monthly','monthly limit'].includes(normalized)?'quota_monthly':null;
      const resetsAt=typeof window.resets_at==='number'&&Number.isFinite(window.resets_at)&&window.resets_at>0?window.resets_at:null;
      return {id:window.id,label:window.label||window.id||'',labelKey,group:window.group||'',used,remaining:100-used,resetsAt,
        stale:account.enabled===false||account.status!=='ok'||!account.fetched_at||now-account.fetched_at>900000||(resetsAt!==null&&resetsAt<=now)};
    });
  }
  function quotaMarkup(account, t, now=Date.now()) {
    const quotas=quotaRows(account,now);
    if(!quotas.length)return `<p class="account-status">${escape(t('no_usage'))}</p>`;
    const locale=t('locale')==='locale'?'en':t('locale');
    const format=value=>new Intl.NumberFormat(locale,{maximumFractionDigits:1}).format(value)+' %';
    return quotas.map(quota=>{
      const label=(quota.group?quota.group+' · ':'')+(quota.labelKey?t(quota.labelKey):quota.label);
      const reset=quota.resetsAt?new Date(quota.resetsAt).toLocaleString(locale):null;
      return `<div class="account-quota${quota.stale?' account-quota-stale':''}">
        <div class="account-quota-title"><strong>${escape(label)}</strong>${quota.stale?`<span>${escape(t('stale'))}</span>`:''}</div>
        <div class="account-quota-figures"><span>${escape(t('used'))}: <strong>${escape(format(quota.used))}</strong></span><span>${escape(t(quota.stale?'remaining_previous':'remaining'))}: <strong>${escape(format(quota.remaining))}</strong></span></div>
        <div class="account-quota-track" role="meter" aria-label="${escape(label)}" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${quota.used.toFixed(1)}"><div class="account-quota-fill" style="width:${quota.used.toFixed(1)}%"></div></div>
        ${reset?`<div class="account-quota-reset">${escape(t('resets'))}: ${escape(reset)}</div>`:''}</div>`;
    }).join('');
  }
  const api = {escape,providerName,cells,quotaRows,quotaMarkup};
  if (typeof module === 'object' && module.exports) module.exports = api;
  else root.UsoaiAccounts = api;
})(typeof globalThis === 'object' ? globalThis : this);
