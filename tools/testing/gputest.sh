for id in "$@"; do
  if [ "$id" = stock ]; then node cdp.mjs eval 'invoke("gpu_stock").then(r=>"stock "+r,e=>"ERR "+e)';
  else node cdp.mjs eval "invoke('profile_switch',{kind:'gpu',id:'$id'}).then(()=>'switched $id',e=>'ERR '+e)"; fi
  sleep 4
  node cdp.mjs eval '(async()=>{const s=await invoke("gpu_state");return JSON.stringify(s.current)})()'
  python hwi.py watch 7 "\| GPU Clock$" "\| GPU Effective Clock$" "\| GPU Memory Clock$" "\| GPU Power$" "\| GPU Core Load$" "\| GPU Temperature$" "\| GPU Thermal Limit$" "Performance Limit - (Power|Thermal)$" | sed 's/.*| //'
done
