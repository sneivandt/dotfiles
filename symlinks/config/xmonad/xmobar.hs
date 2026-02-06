Config {
-- Apearance -------------------------------------------------------------- {{{
      font            = "xft:pixelsize=14:antialias=true:hinting=true:Source Code Pro\
                        \,Font Awesome 5 Free Solid\
                        \,Noto Sans CJK CH\
                        \,Noto Sans CJK JP\
                        \,Noto Sans CJK KR"
    , additionalFonts = [ "Font Awesome 5 Free"
                        , "Font Awesome 5 Free Solid"
                        , "Font Awesome 5 Brands"
                        ]
    , bgColor         = "#121212"
    , fgColor         = "#d0d0d0"
    , alpha           = 240
    , position        = TopH 26
-- }}}
-- Layout ----------------------------------------------------------------- {{{
    , sepChar  = "%"
    , alignSep = "}{"
    , template = " %StdinReader%}{%playing%   %volume%   %date%   %time%  "
-- }}}
-- General ---------------------------------------------------------------- {{{
    , lowerOnStart     = False
    , hideOnStart      = False
    , allDesktops      = True
    , overrideRedirect = False
    , pickBroadest     = False
    , persistent       = True
-- }}}
-- Commands --------------------------------------------------------------- {{{
    , commands =
      [ Run Date "<fn=2></fn> %H:%M"                                                                      "time"    10
      , Run Date "<fn=2></fn> %a %b %d"                                                                   "date"    10
      , Run Com  "bash" ["-c", "$XDG_CONFIG_HOME/xmonad/scripts/playing.sh '<fn=3></fn>'"]                "playing" 60
      , Run Com  "bash" ["-c", "$XDG_CONFIG_HOME/xmonad/scripts/volume.sh  '<fn=2></fn>' '<fn=2></fn>'"] "volume"  60
      , Run StdinReader
      ]
-- }}}
}
