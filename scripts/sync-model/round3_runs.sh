#!/bin/sh
# Third round: the undo of a deletion brings back the entries that the deletion
# had made obsolete (undo_revive). Devices drop obsolete entries after a time and
# write the cut as a floor first (compact), and drop their own entries only once
# the floor is in the shared folder (compact_own_later). The random runs include
# publishes that are interrupted after some of the files.
# compact_nofloor drops entries without a floor, and compact_unpublished writes a
# floor before the device's deletion is in the shared folder; both break.
cd "$(dirname "$0")"
BASE=upload_first,copy_published,task_keep,purge_published,disk_restore,restore_merge_all,keep_deleted
COLL=collections,member_del,coll_delreg,cadd_once,one_tick
NEW=undo_revive,compact,compact_own_later
F='^([A-Za-z0-9-]+: [0-9]+ runs|no violation|random runs|exhaustive|rules|  violated)'
echo "### A the rules of round 2: what an undo loses"
python3 -I model2.py random --runs 100000 --seed 70000000 --procs 14 --rules $BASE,nodisklost 2>&1 | grep -E "$F"
echo "### B undo_revive"
python3 -I model2.py random --runs 400000 --seed 70000000 --procs 14 --rules $BASE,nodisklost,undo_revive 2>&1 | grep -E "$F"
echo "### C undo_revive with compaction without a floor"
python3 -I model2.py random --runs 100000 --seed 71000000 --procs 14 --rules $BASE,nodisklost,undo_revive,compact_nofloor 2>&1 | grep -E "$F"
echo "### C2 a floor written before the deletion is in the shared folder, with disk loss"
python3 -I model2.py random --runs 300000 --seed 76000000 --procs 14 --dense --rules $BASE,purge_lock,$NEW,compact_unpublished 2>&1 | grep -E "$F"
echo "### C3 own entries dropped before the floor is in the shared folder, then an interrupted publish"
python3 -I model2.py trace partial_publish_before_floor --rules $BASE,undo_revive,compact 2>&1 | grep -E "$F"
python3 -I model2.py trace partial_publish_before_floor --rules $BASE,$NEW 2>&1 | grep -E "$F"
echo "### D the new rules, 1,000,000 runs"
python3 -I model2.py random --runs 1000000 --seed 72000000 --procs 14 --stats --rules $BASE,nodisklost,$NEW 2>&1 | cut -c1-2400 | grep -E "$F|^events"
echo "### E the same, dense publishing"
python3 -I model2.py random --runs 400000 --seed 73000000 --procs 14 --dense --rules $BASE,nodisklost,$NEW 2>&1 | grep -E "$F"
echo "### F0 the rules of round 2 with interrupted publishes and disk loss"
python3 -I model2.py random --runs 400000 --seed 74000000 --procs 14 --dense --rules $BASE 2>&1 | grep -E "$F"
echo "### F the new rules with disk loss"
python3 -I model2.py random --runs 400000 --seed 74000000 --procs 14 --dense --rules $BASE,$NEW 2>&1 | grep -E "$F"
echo "### G the same, with collections"
python3 -I model2.py random --runs 400000 --seed 75000000 --procs 14 --dense --rules $BASE,nodisklost,$COLL,$NEW 2>&1 | grep -E "$F"
echo "### H0 the rules of round 2 with purge markers and disk loss"
python3 -I model2.py random --runs 300000 --seed 76000000 --procs 14 --dense --rules $BASE,purge_lock 2>&1 | grep -E "$F"
echo "### H the new rules with purge markers and disk loss"
python3 -I model2.py random --runs 300000 --seed 76000000 --procs 14 --dense --rules $BASE,purge_lock,$NEW 2>&1 | grep -E "$F"
echo "### I the same, wider tag deletion check"
python3 -I model2.py random --runs 300000 --seed 77000000 --procs 14 --dense --tagdel --rules $BASE,nodisklost,tagdel_baseline,$NEW 2>&1 | grep -E "$F"
echo "### J exhaustive items depth 6 full"
python3 -I model2.py exhaust --depth 6 --procs 14 --full --rules $BASE,nodisklost,$NEW 2>&1 | cut -c1-500 | head -8
echo "### K exhaustive collections depth 5"
python3 -I model2.py exhaust --depth 5 --procs 14 --rules $BASE,nodisklost,$COLL,$NEW 2>&1 | cut -c1-500 | head -8
echo "### done"
