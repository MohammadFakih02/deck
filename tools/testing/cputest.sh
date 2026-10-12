for id in "$@"; do
  node cdp.mjs eval "invoke('profile_switch',{kind:'cpu',id:'$id'}).then(()=>'switched $id',e=>'ERR '+e)"
  sleep 2
  python load.py 18 & 
  sleep 6
  python hwi.py watch 8 "^P-core 0 Clock|^E-core 8 Clock|Average Effective Clock|CPU Package Power|PL1 Power Limit \(Static\)|PL2 Power Limit \(Static\)|CPU Package =" 2>&1 | sed 's/.*| //'
  wait
done
