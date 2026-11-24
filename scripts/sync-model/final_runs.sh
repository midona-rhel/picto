#!/bin/sh
cd "$(dirname "$0")"
FULL=join_republish,task_keep,upload_first,restore_newid,purge_published,purge_lock
echo "### 1 exhaustive depth 6, full action set, final rules"; date
python3 -I model.py exhaust --depth 6 --procs 14 --full --rules $FULL 2>&1 | cut -c1-700 | head -40
echo "### 2 random 1,000,000, final rules"; date
python3 -I model.py random --runs 1000000 --seed 10000000 --procs 14 --stats --rules $FULL 2>&1 | cut -c1-1600 | head -60
echo "### 3 random 600,000, final rules + tagdel_baseline, wider P5 check on"; date
python3 -I model.py random --runs 600000 --seed 20000000 --procs 14 --tagdel --rules $FULL,tagdel_baseline 2>&1 | cut -c1-1600 | head -60
echo "### 4 exhaustive depth 6, full action set, final rules + tagdel_baseline, wider P5 check on"; date
python3 -I model.py exhaust --depth 6 --procs 14 --full --tagdel --rules $FULL,tagdel_baseline 2>&1 | cut -c1-700 | head -40
echo "### 5 exhaustive depth 7, core action set, final rules"; date
python3 -I model.py exhaust --depth 7 --procs 14 --rules $FULL 2>&1 | cut -c1-700 | head -40
echo "### done"; date
