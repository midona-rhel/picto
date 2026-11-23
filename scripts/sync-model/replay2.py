#!/usr/bin/env python3
"""Second round: replays the shortest trace of each finding with model2.py.
Run: python3 -I replay2.py"""
import os
import sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import model2 as m

fs = frozenset
E2 = ((0, None, ()), (0, None, ()))
E3 = ((0, None, ()), (0, None, ()), (0, None, ()))


def rec(name, notes, rating, status, tags, folders, coll=None):
    return (name, notes, rating, status, fs(tags), fs(folders), coll)


def crec(name, notes, rating, status, tags, folders):
    return (name, notes, rating, status, fs(tags), fs(folders))


V2C2 = 'upload_first,join_republish,task_keep,restore_newid,purge_published,disk_restore'
V2V1 = 'upload_first,copy_published,task_keep,restore_newid,purge_published,disk_restore'
V3 = 'upload_first,copy_published,task_keep,purge_published,disk_restore,restore_merge_all'
BASE = V3 + ',keep_deleted'
TRACES = [
    ('V2 with C2 kept: a backup holds an unpublished add, then the disk is lost', V2C2,
     ((0, None, ()), (0, None, ()), (0, (('h1', rec('nC', 'y', 1, 'inbox', {'t1'}, ())),), ())), 1,
     [('start', 2), ('snapshot', 2), ('disklost', 2)], False),
    ('V2 with V1 and C4: purge round overlaps an add, then the adding device loses its disk', V2V1,
     ((0, None, ()), (0, (('h1', rec('nC', 'y', 0, 'inbox', {'t1', 't2'}, ())),), ()), (0, None, ())), 1,
     [('start', 1), ('publish', 1), ('delete', 1, 'h1'), ('publish', 1), ('wait', 5),
      ('purge_begin', 1), ('import', 0, 'h1', rec('nC', 'x', 1, 'inbox', (), {'f1'})),
      ('publish', 0), ('disklost', 0)], False),
    ('V2 with V1 and C4, no disk loss: the restored device merges an older delete before its '
     'own published add', V2V1 + ',nodisklost',
     ((0, (('h1', rec('nC', 'y', 1, 'inbox', {'t1'}, {'f1'})),), ()), (25, None, ()),
      (0, (('h1', rec('nC', 'y', 1, 'inbox', {'t2'}, ())),), ())), 1,
     [('start', 0), ('snapshot', 0), ('start', 2), ('delete', 2, 'h1'), ('snapshot', 1),
      ('publish', 2), ('merge', 0, 2, None),
      ('import', 1, 'h1', rec('nA', '', 2, 'inbox', {'t1'}, ())), ('wait', 5),
      ('purge_begin', 2), ('publish', 1), ('restore', 1), ('merge', 1, 0, None),
      ('merge', 1, 2, None)], False),
    ('V3: purge round overlaps an add, then the adding device loses its disk', V3,
     ((0, None, ()), (0, None, ()), (0, (('h1', rec('nB', 'x', 1, 'active', {'t2'}, {'f1'})),), ())), 1,
     [('import', 0, 'h1', rec('nA', 'x', 0, 'inbox', (), {'f1'})), ('publish', 0),
      ('delete', 0, 'h1'), ('publish', 0), ('wait', 5), ('purge_begin', 0), ('start', 2),
      ('publish', 2), ('disklost', 2)], False),
    ('V3, no disk loss: a local delete removes the file, a restore removes the delete',
     V3 + ',nodisklost', E3, 1,
     [('import', 2, 'h1', rec('nA', '', 0, 'inbox', {'t2'}, ())), ('delete', 2, 'h1'),
      ('tagdel', 1, 't2'),
      ('import', 1, 'h1', rec('nC', '', 2, 'inbox', {'t1'}, {'f1'})), ('publish', 1),
      ('delete', 1, 'h1'), ('publish', 1), ('wait', 5), ('snapshot', 2), ('purge_begin', 1),
      ('undo', 2, 'h1'), ('publish', 2), ('delete', 2, 'h1'), ('restore', 2)], False),
    ('G1 a device replaces its own collection_add',
     BASE + ',nodisklost,collections,member_del,coll_delreg,one_tick', ((0, None, ()),), 2,
     [('ipost', 0, 'p1', (('h2', rec('nA', '', 0, 'inbox', {'t2'}, {'f1'})),),
       crec('nB', '', 0, 'active', {'t1'}, {'f1'})),
      ('ungroup', 0, 'p1'),
      ('ipost', 0, 'p1', (('h1', rec('nC', 'x', 1, 'inbox', {'t1'}, ())),),
       crec('nA', 'y', 1, 'active', (), {'f1'}))], False),
    ('G2 a member entry older than the deletion decides for the file that was added again',
     BASE + ',nodisklost,collections,coll_delreg,cadd_once,one_tick',
     ((0, None, ()), (0, None, ()),
      (4, (('h1', rec('nB', 'y', 2, 'active', (), (), 'g0')),),
       (('g0', crec('nA', '', 0, 'inbox', (), ())),))), 2,
     [('ipost', 1, 'p2', (('h1', rec('nC', 'x', 1, 'inbox', {'t1'}, {'f1'})),
                          ('h2', rec('nC', '', 1, 'active', (), ()))),
       crec('nC', 'y', 0, 'active', (), {'f1'})),
      ('detach', 1, 'h1'), ('delete', 1, 'h1')], False),
    ('G3 a collection that was deleted permanently exists again through an older member',
     BASE + ',nodisklost,collections,member_del,cadd_once,one_tick',
     ((0, None, ()), (0, (('h2', rec('nC', 'y', 0, 'inbox', (), (), 'p1')),),
                      (('p1', crec('nA', '', 0, 'inbox', (), ())),)), (0, None, ())), 2,
     [('ipost', 2, 'p1', (('h1', rec('nC', '', 2, 'inbox', (), ())),),
       crec('nC', 'y', 1, 'inbox', {'t2'}, ())),
      ('wait', 1), ('cdelete', 2, 'p1')], False),
    ('G4 two devices group the same files at the same time (one time per entry)',
     BASE + ',nodisklost,collections,member_del,coll_delreg,cadd_once',
     E2, 2, m.SCRIPTS['split_grouping'][2], False),
    ('Observation: one file in two different collections on two older copies',
     BASE + ',nodisklost,collections,member_del,coll_delreg,cadd_once,one_tick',
     ((0, (('h2', rec('nA', 'x', 1, 'inbox', {'t1'}, (), 'g0')),),
       (('g0', crec('nA', '', 0, 'inbox', (), ())),)),
      (0, (('h2', rec('nB', 'y', 2, 'inbox', (), (), 'p1')),),
       (('p1', crec('nC', 'x', 0, 'inbox', {'t1'}, ())),))), 2, [], True),
]
only = sys.argv[1:]
for name, rules, init, nfiles, actions, vanish in TRACES:
    if only and not any(x in name for x in only):
        continue
    m.CHECK_VANISH = vanish
    r = m.Rules(tuple(x for x in rules.split(',') if x))
    print('=== %s; rules: %r' % (name, r))
    m.replay_print(r, init, nfiles, actions)
    print()
