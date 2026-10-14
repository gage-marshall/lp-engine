"""The ported functions by game address (what a context runs in Python instead of treating as external)."""
from . import top, screens


def ports(only=None):
    table = {
        0x203fb20: ('Main_ScopedFadeCheck', top.scoped_fade_check),
        0x204b0c0: ('Top_Run', top.run),
        0x204afdc: ('Top_BootOverlays', top.boot_overlays),
        0x2089d8c: ('Screen_Request', screens.request),
        0x2089e04: ('Screen_Run', screens.run),
        0x208a2b4: ('Screen_Transition', screens.transition),
        0x208a994: ('Screen_LoadOverlay', screens.load_overlay),
        0x208abd4: ('Screen_UnloadOverlay', screens.unload_overlay),
    }
    try:
        from . import scene
        table[0x204a0c4] = ('Scene_Run', scene.run)
    except ImportError:
        pass
    try:
        from . import saveselect
        table.update({0x2074600: ('SaveSelect_Run', saveselect.run),
                      0x209f4cc: ('SceneLoopA_Init', saveselect.loop_a_init),
                      0x209f50c: ('SceneLoopA_Run', saveselect.loop_a_run),
                      0x209f618: ('SceneLoopB_Init', saveselect.loop_b_init),
                      0x209f658: ('SceneLoopB_Run', saveselect.loop_b_run)})
    except ImportError:
        pass
    if only is not None:
        table = {a: v for a, v in table.items() if v[0] in only}
    return table
