#!/bin/sh
# Third round: the undo of a deletion brings back the entries it had made obsolete
# (undo_revive), and devices drop obsolete entries after a time, writing a floor
# first (compact). compact_nofloor drops them without a floor and shows why the
# floor is needed.
cd "$(dirname "$0")"
BASE=upload_first,copy_published,task_keep,purge_published,disk_restore,restore_merge_all,keep_deleted
COLL=collections,member_del,coll_delreg,cadd_once,one_tick
F='^(P[0-9]|no violation|random runs|exhaustive|rules)'
echo "### A the rules of round 2: what an undo loses"
python3 -I model2.py random --runs 100000 --seed 70000000 --procs 14 --rules $BASE,nodisklost 2>&1 | grep -E "$F"
echo "### B undo_revive"
python3 -I model2.py random --runs 400000 --seed 70000000 --procs 14 --rules $BASE,nodisklost,undo_revive 2>&1 | grep -E "$F"
echo "### C2 undo_revive and compact, but a floor may rest on an unpublished deletion"
python3 -I model2.py random --runs 300000 --seed 76000000 --procs 14 --dense --rules $BASE,purge_lock,undo_revive,compact,compact_unpublished 2>&1 | grep -E "$F"
echo "### C undo_revive with compaction without a floor"
python3 -I model2.py random --runs 100000 --seed 71000000 --procs 14 --rules $BASE,nodisklost,undo_revive,compact_nofloor 2>&1 | grep -E "$F"
echo "### D undo_revive and compact, 1,000,000 runs"
python3 -I model2.py random --runs 1000000 --seed 72000000 --procs 14 --stats --rules $BASE,nodisklost,undo_revive,compact 2>&1 | cut -c1-2200 | grep -E "$F|^events"
echo "### E the same, dense publishing"
python3 -I model2.py random --runs 400000 --seed 73000000 --procs 14 --dense --rules $BASE,nodisklost,undo_revive,compact 2>&1 | grep -E "$F"
echo "### F the same, with disk loss"
python3 -I model2.py random --runs 400000 --seed 74000000 --procs 14 --dense --rules $BASE,undo_revive,compact 2>&1 | grep -E "$F"
echo "### G the same, with collections"
python3 -I model2.py random --runs 400000 --seed 75000000 --procs 14 --dense --rules $BASE,nodisklost,$COLL,undo_revive,compact 2>&1 | grep -E "$F"
echo "### H the same, with purge markers and disk loss"
python3 -I model2.py random --runs 300000 --seed 76000000 --procs 14 --dense --rules $BASE,purge_lock,undo_revive,compact 2>&1 | grep -E "$F"
echo "### I the same, wider tag deletion check"
python3 -I model2.py random --runs 300000 --seed 77000000 --procs 14 --dense --tagdel --rules $BASE,nodisklost,tagdel_baseline,undo_revive,compact 2>&1 | grep -E "$F"
echo "### J exhaustive items depth 6 full"
python3 -I model2.py exhaust --depth 6 --procs 14 --full --rules $BASE,nodisklost,undo_revive,compact 2>&1 | cut -c1-500 | head -8
echo "### K exhaustive collections depth 5"
python3 -I model2.py exhaust --depth 5 --procs 14 --rules $BASE,nodisklost,$COLL,undo_revive,compact 2>&1 | cut -c1-500 | head -8
echo "### done"
