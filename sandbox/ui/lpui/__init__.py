"""LovePlus+ UI framework (sandbox): the game's 2D screens as data (file loads into VRAM, display registers, OAM, touch
areas) plus the logic that changes them, rendered by the DS 2D compositor (sandbox/ui/ds2d.py).

    vram.Vram          both engines' VRAM / palettes / OAM / registers; apply(load op) from the game's files
    touch.TouchCtl     the game's hit lists (HitList_Add items, TouchCtl_PollPress / Input_UpdateHeldState rule)
    screens            screen recipes and the transition graph extracted from a replay trace
    runtime.Runtime    current screen, tap -> hit -> transition, render
"""
