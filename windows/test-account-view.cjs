const {test}=require('node:test');
const assert=require('node:assert/strict');
const {cells,escape,quotaRows,quotaMarkup}=require('./codenotch/ui/account-view.js');

test('two accounts of one provider keep distinct readings, aliases and retry deadlines',()=>{
  const records=[
    {account_id:'codex-work',provider:'codex',label:'Work',snapshot:{status:'ok',windows:[{id:'primary',used:.2}],backoff_until:0}},
    {account_id:'codex-home',provider:'codex',label:'Home',snapshot:{status:'backoff',windows:[{id:'primary',used:.9}],backoff_until:900}}
  ];
  const result=cells(records,[{id:'codex-work',label:'Trabajo',enabled:true},{id:'codex-home',label:'Personal',enabled:true}]);
  assert.equal(result.length,2);
  assert.equal(result[0].id,'codex-work');assert.equal(result[1].id,'codex-home');
  assert.equal(result[0].snap.windows[0].used,.2);assert.equal(result[1].snap.windows[0].used,.9);
  assert.equal(result[0].name,'Codex · Trabajo');assert.equal(result[1].snap.backoff_until,900);
});

test('settings show used and remaining quota for each real window, including zero',()=>{
  const now=1800000000000;
  const account={enabled:true,status:'ok',fetched_at:now,windows:[
    {id:'primary',label:'5h limit',used:.01,resets_at:now+3600000},
    {id:'secondary',label:'Weekly limit',used:.15,resets_at:now+86400000},
    {id:'empty',label:'Additional window',used:0}
  ]};
  const rows=quotaRows(account,now);
  assert.deepEqual(rows.map(row=>[row.used,row.remaining]),[[1,99],[15,85],[0,100]]);
  assert.deepEqual(rows.map(row=>row.labelKey),['quota_5h','quota_weekly',null]);
  const labels={locale:'es',quota_5h:'Límite de 5 horas',quota_weekly:'Límite semanal',used:'Usado',remaining:'Disponible',resets:'Se renueva'};
  const html=quotaMarkup(account,key=>labels[key]||key,now);
  const visible=html.replace(/<[^>]*>/g,' ').replace(/\s+/g,' ');
  assert.match(visible,/Límite de 5 horas/);
  assert.match(visible,/Usado: 1 %/);assert.match(visible,/Disponible: 99 %/);
  assert.match(visible,/Límite semanal/);assert.match(visible,/Disponible: 85 %/);
  assert.match(visible,/Usado: 0 %/);assert.match(visible,/Se renueva:/);
});

test('missing quotas never become a fictitious unused allowance and old data are marked',()=>{
  const now=1800000000000;
  assert.equal(quotaRows({status:'needsAuth'},now).length,0);
  assert.ok(!quotaMarkup({status:'needsAuth'},key=>key,now).includes('100 %'));
  const expired={enabled:true,status:'ok',fetched_at:now,windows:[{id:'primary',label:'5h limit',used:.5,resets_at:now-1}]};
  assert.equal(quotaRows(expired,now)[0].stale,true);
  const html=quotaMarkup(expired,key=>key==='remaining_previous'?'Según la última lectura':key,now);
  assert.match(html,/Según la última lectura/);
  assert.match(html,/account-quota-stale/);
});

test('invalid quota values and vendor labels cannot inject markup or false percentages',()=>{
  const account={status:'ok',fetched_at:1800000000000,windows:[
    {id:'bad',used:NaN},{id:'count',used:.5,count:10},{id:'text',used:'0.3'},
    {id:'valid',used:1.5,label:'<img src=x onerror=alert(1)>',group:'<script>'}
  ]};
  const rows=quotaRows(account,1800000000000);
  assert.equal(rows.length,1);assert.equal(rows[0].used,100);assert.equal(rows[0].remaining,0);
  const html=quotaMarkup(account,key=>key,1800000000000);
  assert.ok(!html.includes('<img'));assert.ok(!html.includes('<script>'));
});

test('paused and absent accounts disappear without hiding another account',()=>{
  const records=[
    {account_id:'opencode',provider:'opencode',snapshot:{status:'absent'}},
    {account_id:'opencode-work',provider:'opencode',snapshot:{status:'ok'}},
    {account_id:'opencode-home',provider:'opencode',snapshot:{status:'needsAuth'}}
  ];
  assert.deepEqual(cells(records,[{id:'opencode-work',enabled:false},{id:'opencode-home',enabled:true}]).map(x=>x.id),['opencode-home']);
});

test('labels cannot inject HTML and credential metadata is not copied into usage cells',()=>{
  const label='<img src=x onerror="alert(1)">';
  assert.equal(escape(label),'&lt;img src=x onerror=&quot;alert(1)&quot;&gt;');
  const result=cells([{account_id:'opencode-work',provider:'opencode',snapshot:{status:'ok'}}],
    [{id:'opencode-work',label,enabled:true,source:{kind:'secret',key:'usoai/opencode-work'},credential:'must-never-reach-the-view'}]);
  assert.ok(!JSON.stringify(result).includes('must-never-reach-the-view'));
  assert.ok(!JSON.stringify(result).includes('usoai/opencode-work'));
});
