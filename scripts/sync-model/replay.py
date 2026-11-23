#!/usr/bin/env python3
"""Replays the shortest trace of each flaw under the rule set in which it was found.
Run: python3 -I replay.py"""
import os
import sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import model as m

fs = frozenset
E2 = ((0, None), (0, None))
E3 = ((0, None), (0, None), (0, None))
GIVEN = ''
TRACES = [
    ('F1 join copies unpublished entries, source is restored', GIVEN, E2, 1, [
        ('snapshot', 0), ('import', 0, 'h1', ('nA', '', 0, 'inbox', fs({'t1'}), fs())),
        ('join', 0, False), ('restore', 0)]),
    ('F1 variant: an explicit folder removal stays on the joined device only', GIVEN, E3, 2, [
        ('import', 2, 'h2', ('nC', 'y', 2, 'inbox', fs({'t2'}), fs({'f1'}))), ('publish', 2),
        ('upload', 2), ('merge', 0, 2, None), ('snapshot', 0),
        ('folder', 0, 'h2', 'f1', 'remove'), ('join', 0, False), ('restore', 0)]),
    ('F2a two tasks with the same file, the first is undone', GIVEN, ((0, None),), 1, [
        ('tbegin', 0, (('h1', ('nB', 'x', 1, 'inbox', fs(), fs())),)),
        ('tbegin', 0, (('h1', ('nC', '', 0, 'active', fs({'t2'}), fs({'f1'}))),)),
        ('tundo', 0, 0)]),
    ('F2b single import of a file of an unfinished task, the task is undone', GIVEN, E2, 1, [
        ('tbegin', 0, (('h1', ('nA', '', 0, 'inbox', fs({'t1'}), fs())),)),
        ('import', 0, 'h1', ('nA', '', 0, 'inbox', fs({'t1'}), fs())), ('tundo', 0, 0)]),
    ('F2c copy drops the file of an unfinished task although an add exists', GIVEN, E2, 1, [
        ('tbegin', 1, (('h1', ('nA', '', 1, 'inbox', fs({'t1', 't2'}), fs({'f1'}))),)),
        ('import', 1, 'h1', ('nA', '', 1, 'inbox', fs({'t1'}), fs())),
        ('snapshot', 1), ('restore', 1)]),
    ('F3 entries are published before the original is uploaded', GIVEN, E3, 1, [
        ('snapshot', 2), ('import', 2, 'h1', ('nB', '', 1, 'inbox', fs({'t2'}), fs())),
        ('publish', 2), ('restore', 2)]),
    ('F4a restored device writes its add key again', GIVEN, E2, 1, [
        ('snapshot', 1), ('import', 1, 'h1', ('nA', 'y', 0, 'inbox', fs({'t1', 't2'}), fs({'f1'}))),
        ('upload', 1), ('publish', 1), ('restore', 1),
        ('import', 1, 'h1', ('nA', 'x', 0, 'inbox', fs({'t2'}), fs()))]),
    ('F4b the same with "merge own published entries first" applied',
     'join_republish,task_keep,upload_first,restore_merge', E3, 2, [
        ('snapshot', 2), ('import', 2, 'h2', ('nC', '', 0, 'inbox', fs({'t1', 't2'}), fs({'f1'}))),
        ('join', 2, False), ('restore', 2), ('merge', 2, 2, None),
        ('import', 2, 'h2', ('nB', 'y', 1, 'active', fs({'t2'}), fs()))]),
    ('F5 purge round overlaps a new add, then the adding device is restored',
     'join_republish,task_keep,upload_first,restore_newid,purge_published', E3, 2, [
        ('import', 0, 'h2', ('nA', 'x', 2, 'inbox', fs({'t1', 't2'}), fs())), ('publish', 0),
        ('delete', 0, 'h2'), ('publish', 0), ('wait', 6), ('snapshot', 1), ('purge_begin', 0),
        ('import', 1, 'h2', ('nC', 'y', 0, 'inbox', fs({'t1'}), fs())), ('publish', 1),
        ('restore', 1)]),
]
for name, rules, init, nfiles, actions in TRACES:
    r = m.Rules(tuple(x for x in rules.split(',') if x))
    print('=== %s; rules: %r' % (name, r))
    m.replay_print(r, init, nfiles, actions)
    print()
