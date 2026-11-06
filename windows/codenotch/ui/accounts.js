/* Account values are escaped; API keys only travel into the add command and are cleared immediately. */
(() => {
  'use strict';
  const root = document.getElementById('multi-account-root');
  if (!root || !window.__TAURI__) return;
  const invoke = window.__TAURI__.core.invoke;
  const esc = window.UsoaiAccounts.escape;
  const providerName = window.UsoaiAccounts.providerName;
  const quotaMarkup = window.UsoaiAccounts.quotaMarkup;
  let strings = {}, accounts = [], candidates = [], busy = false, codexLogins = {}, claudeAuth = {};
  const t = key => strings[key] || key;
  const byId = id => document.getElementById('account-'+id);
  const message = (text, error=false) => { const node = byId('message'); if(node){node.textContent=text;node.dataset.error=String(error);} };
  function sourceLabel(account) {
    const source = account.source || {};
    if(source.kind === 'config_dir') return source.path;
    if(source.kind === 'env_var') return t('variable')+': '+source.name;
    if(source.kind === 'secret') return t('secure_storage');
    return t('default_profile');
  }
  function status(account) {
    if(!account.enabled) return t('paused');
    const login = account.provider==='codex' ? codexLogins[account.id] : null;
    if(login?.busy) return t('finish_login');
    const labels = {ok:'connected',stale:'stale',needsAuth:'needs_login',absent:'not_found',backoff:'backoff',error:'read_error',none:'waiting'};
    return t(labels[account.status] || 'waiting');
  }
  function row(account) {
    const login = codexLogins[account.id];
    const claudeId=account.id==='claude'?'default':account.id.replace(/^claude-/,'');
    const loginBusy = account.provider==='codex' ? !!login?.busy : !!claudeAuth.accounts?.[claudeId]?.busy;
    const connect = account.provider!=='opencode' && ['default','config_dir'].includes(account.source?.kind);
    return `<div class="account-card" data-account-id="${esc(account.id)}">
      <div class="account-heading"><strong>${esc(providerName(account.provider))}</strong><input class="account-label" maxlength="40" value="${esc(account.label)}" aria-label="${esc(t('name'))}"></div>
      <div class="account-status account-reading-status">${esc(status(account))}${account.fetched_at?' · '+esc(new Date(account.fetched_at).toLocaleString()):''}</div>
      <div class="account-quotas">${quotaMarkup(account,t)}</div>
      <div class="account-source">${esc(sourceLabel(account))}</div>
      <div class="account-actions"><label><input type="checkbox" class="account-enabled" ${account.enabled?'checked':''}> ${esc(t('monitor'))}</label>
      <button class="btn" type="button" data-action="save">${esc(t('save'))}</button>
      ${connect?`<button class="btn" type="button" data-action="connect" ${loginBusy?'disabled':''}>${esc(t('connect'))}</button>`:''}
      ${account.id!==account.provider?`<button class="btn" type="button" data-action="remove">${esc(t('remove'))}</button>`:''}</div>
      ${login?.message&&(login.busy||account.status!=='ok')?`<div class="account-status">${esc(login.busy?t('finish_login'):t('login_finished'))}</div>`:''}</div>`;
  }
  function drawList() {
    const active = document.activeElement;
    if(active && byId('list').contains(active) && active.matches('input[type=text],input:not([type])')) {
      for(const row of byId('list').querySelectorAll('[data-account-id]')) {
        const account=accounts.find(account=>account.id===row.dataset.accountId);
        if(!account)continue;
        row.querySelector('.account-quotas').innerHTML=quotaMarkup(account,t);
        row.querySelector('.account-reading-status').textContent=status(account)+(account.fetched_at?' · '+new Date(account.fetched_at).toLocaleString():'');
      }
      return;
    }
    byId('list').innerHTML=accounts.map(row).join('');
    lock();
  }
  function lock() {
    root.querySelectorAll('button,input,select').forEach(node=>{
      if(busy){if(node.dataset.wasDisabled===undefined)node.dataset.wasDisabled=String(node.disabled);node.disabled=true;}
      else if(node.dataset.wasDisabled!==undefined){node.disabled=node.dataset.wasDisabled==='true';delete node.dataset.wasDisabled;}
    });
  }
  function methodFields() {
    const provider=byId('provider').value;
    const selected=byId('method').value;
    const choices=provider==='opencode'?[['folder','folder'],['token','api_key'],['env','variable']]:[['login','official_login'],['folder','folder']];
    byId('method').innerHTML=choices.map(([value,key])=>`<option value="${value}">${esc(t(key))}</option>`).join('');
    if(choices.some(([value])=>value===selected))byId('method').value=selected;
    fields();
  }
  function fields() {
    const method=byId('method').value;
    for(const name of ['path','credential','variable']) {
      const shown=method===({path:'folder',credential:'token',variable:'env'})[name];
      byId(name+'-field').hidden=!shown;byId(name).required=shown;
    }
    if(method!=='token')byId('credential').value='';
  }
  async function refresh() {
    const values=await Promise.all([invoke('list_accounts'),invoke('get_codex_logins'),invoke('get_claude_auth')]);
    accounts=values[0]||[];codexLogins=values[1]||{};claudeAuth=values[2]||{};drawList();
  }
  async function operation(action) {
    if(busy)return;busy=true;lock();message('');
    try {await action();await refresh();}
    catch(error){message(typeof error==='string'?error:String(error),true);}
    finally{busy=false;lock();}
  }
  async function boot() {
    strings=await invoke('account_ui_strings');
    root.innerHTML=`<div class="sec">${esc(t('title'))}</div>
      <div class="account-toolbar"><button class="btn" id="account-add">${esc(t('add'))}</button><button class="btn" id="account-detect">${esc(t('detect'))}</button></div>
      <p class="account-message" id="account-message" role="status" aria-live="polite"></p>
      <form class="group account-form" id="account-form" hidden>
        <label class="account-field">${esc(t('provider'))}<select id="account-provider"><option value="claude">Claude</option><option value="codex">Codex</option><option value="opencode">OpenCode</option></select></label>
        <label class="account-field">${esc(t('name'))}<input id="account-name" maxlength="40" required autocomplete="off"></label>
        <label class="account-field">${esc(t('method'))}<select id="account-method"></select></label>
        <label class="account-field" id="account-path-field">${esc(t('path'))}<input id="account-path" autocomplete="off" placeholder="C:\\Users\\…"></label>
        <label class="account-field" id="account-credential-field">${esc(t('api_key'))}<input id="account-credential" type="password" autocomplete="off" spellcheck="false"></label>
        <label class="account-field" id="account-variable-field">${esc(t('variable'))}<input id="account-variable" autocomplete="off" placeholder="OPENCODE_APIKEY"></label>
        <p class="account-status">${esc(t('login_help'))}</p>
        <div class="account-actions"><button class="btn" type="submit">${esc(t('save'))}</button><button class="btn" type="button" id="account-cancel">${esc(t('cancel'))}</button></div>
      </form>
      <div class="group" id="account-candidates" hidden></div>
      <div class="group" id="account-list"></div>`;
    methodFields();
    byId('provider').addEventListener('change',methodFields);byId('method').addEventListener('change',fields);
    byId('add').addEventListener('click',()=>{byId('form').hidden=false;byId('name').focus();});
    byId('cancel').addEventListener('click',()=>{byId('credential').value='';byId('form').hidden=true;});
    byId('form').addEventListener('submit',event=>{
      event.preventDefault();
      const payload={provider:byId('provider').value,label:byId('name').value,method:byId('method').value,path:byId('path').value||null,credential:byId('credential').value||null,variable:byId('variable').value||null};
      byId('credential').value='';
      operation(async()=>{let id;try{id=await invoke('add_account',payload);}finally{payload.credential=null;}byId('form').hidden=true;
        if(payload.method==='login'){await invoke('connect_account',{id});message(t('finish_login'));}else message(t('saved'));});
    });
    byId('list').addEventListener('click',event=>{
      const button=event.target.closest('[data-action]');if(!button||busy)return;
      const element=button.closest('[data-account-id]');const id=element.dataset.accountId;
      if(button.dataset.action==='remove'&&!window.confirm(t('remove_confirm')))return;
      operation(async()=>{
        if(button.dataset.action==='save')await invoke('update_account',{id,label:element.querySelector('.account-label').value,enabled:element.querySelector('.account-enabled').checked});
        if(button.dataset.action==='remove')await invoke('remove_account',{id});
        if(button.dataset.action==='connect'){await invoke('connect_account',{id});message(t('finish_login'));}
      });
    });
    byId('list').addEventListener('change',event=>{
      if(!event.target.matches('.account-enabled'))return;
      const element=event.target.closest('[data-account-id]');
      operation(()=>invoke('update_account',{id:element.dataset.accountId,label:element.querySelector('.account-label').value,enabled:event.target.checked}));
    });
    byId('detect').addEventListener('click',()=>operation(async()=>{
      candidates=await invoke('detect_accounts');byId('candidates').hidden=!candidates.length;
      byId('candidates').innerHTML=candidates.map((candidate,index)=>`<label class="candidate-row"><input type="checkbox" value="${index}"><span>${esc(providerName(candidate.provider))} · ${esc(candidate.label)}<br>${esc(candidate.path)}</span></label>`).join('')+`<div class="account-toolbar"><button class="btn" type="button" id="account-import">${esc(t('import'))}</button></div>`;
      if(!candidates.length)message(t('no_candidates'));
    }));
    byId('candidates').addEventListener('click',event=>{
      if(event.target.id!=='account-import')return;
      const selected=[...byId('candidates').querySelectorAll('input:checked')].map(input=>candidates[Number(input.value)]);
      operation(async()=>{for(const candidate of selected)await invoke('add_account',{provider:candidate.provider,label:candidate.label.slice(0,40),method:'folder',path:candidate.path,credential:null,variable:null});byId('candidates').hidden=true;message(t('saved'));});
    });
    await refresh();
    window.__TAURI__.event.listen('accounts_changed',()=>{refresh().catch(error=>message(String(error),true));});
    setInterval(()=>{if(!busy)refresh().catch(()=>{});},10000);
  }
  boot().catch(error=>{root.textContent=String(error);});
})();
