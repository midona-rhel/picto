#!/bin/sh
cd "$(dirname "$0")"
BASE=upload_first,copy_published,task_keep,purge_published,disk_restore,restore_merge_all,keep_deleted
COLL=collections,member_del,coll_delreg,cadd_once,one_tick
F='^(P[0-9]|no violation|random runs|exhaustive|rules)'
for drop in member_del coll_delreg cadd_once one_tick; do
  R=$(echo $COLL | tr ',' '\n' | grep -v "^$drop$" | paste -sd, -)
  echo "### A without $drop"
  python3 -I model2.py random --runs 150000 --seed 50000000 --procs 14 --dense --rules $BASE,nodisklost,$R 2>&1 | grep -E "$F"
done
echo "### B vanish observation"
python3 -I model2.py random --runs 100000 --seed 51000000 --procs 14 --dense --vanish --only P2-vanish --shrink 12 --rules $BASE,nodisklost,$COLL 2>&1 | grep -v "^events" | cut -c1-900 | head -40
echo "### C1 final random 1,000,000, base + collections, disk loss off"
python3 -I model2.py random --runs 1000000 --seed 60000000 --procs 14 --stats --rules $BASE,nodisklost,$COLL 2>&1 | cut -c1-2200 | head -20
echo "### C2 final random 600,000 dense, base + collections, disk loss off"
python3 -I model2.py random --runs 600000 --seed 61000000 --procs 14 --dense --rules $BASE,nodisklost,$COLL 2>&1 | grep -E "$F"
echo "### D base + collections with disk loss"
python3 -I model2.py random --runs 400000 --seed 62000000 --procs 14 --dense --rules $BASE,$COLL 2>&1 | grep -E "$F"
echo "### E base + C5 markers + collections with disk loss"
python3 -I model2.py random --runs 400000 --seed 63000000 --procs 14 --dense --rules $BASE,purge_lock,$COLL 2>&1 | grep -E "$F"
echo "### F items only, base + one time per action, disk loss off"
python3 -I model2.py random --runs 400000 --seed 64000000 --procs 14 --rules $BASE,nodisklost,one_tick 2>&1 | grep -E "$F"
echo "### I base + collections + C7, wider P5 check"
python3 -I model2.py random --runs 300000 --seed 65000000 --procs 14 --dense --tagdel --rules $BASE,nodisklost,$COLL,tagdel_baseline 2>&1 | grep -E "$F"
echo "### H1 exhaustive items depth 6 full, base, disk loss off"
python3 -I model2.py exhaust --depth 6 --procs 14 --full --rules $BASE,nodisklost 2>&1 | cut -c1-500 | head -8
echo "### G exhaustive collections depth 5"
python3 -I model2.py exhaust --depth 5 --procs 14 --rules $BASE,nodisklost,$COLL 2>&1 | cut -c1-500 | head -8
echo "### J items only, base without keep_deleted, disk loss off (the V3 set as first tested)"
python3 -I model2.py random --runs 1000000 --seed 66000000 --procs 14 --rules upload_first,copy_published,task_keep,purge_published,disk_restore,restore_merge_all,nodisklost 2>&1 | grep -E "$F"
echo "### done"
