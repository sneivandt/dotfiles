-- Imports ---------------------------------------------------------------- {{{
import XMonad
import XMonad.Actions.CycleWS
import XMonad.Config.Desktop
import XMonad.Hooks.DynamicLog
import XMonad.Hooks.EwmhDesktops hiding (fullscreenEventHook)
import XMonad.Hooks.ManageDocks
import XMonad.Layout.Fullscreen
import XMonad.Layout.Grid
import XMonad.Layout.MultiToggle
import XMonad.Layout.MultiToggle.Instances
import XMonad.Layout.NoBorders
import XMonad.Layout.Reflect
import XMonad.Layout.Renamed
import XMonad.Layout.Spacing
import XMonad.Operations
import XMonad.Util.EZConfig
import XMonad.Util.NamedScratchpad
import XMonad.Util.Run
import qualified XMonad.StackSet as W
import qualified Data.Map as M
import Control.Monad (when)
import Graphics.X11.Xlib.Extras
import Foreign.C.Types (CLong)
import Data.List (isInfixOf)
-- }}}
-- Theme ------------------------------------------------------------------ {{{
myBorderWidth        = 3
myNormalBorderColor  = "#1a1a1a"
myFocusedBorderColor = "#61afef"

myDmenuFont          = "xft:Source Code Pro:pixelsize=14:antialias=true:hinting=true"
myDmenuNormBG        = "#121212"
myDmenuSelBG         = "#3465a4"
myDmenuNormFG        = "#d0d0d0"
myDmenuSelFG         = "#d0d0d0"
-- }}}
-- Main ------------------------------------------------------------------- {{{
main = do
  wsBar <- spawnPipe myWsBar
  xmonad
    $ ewmh
    $ fullscreenSupport
    $ docks
    $ desktopConfig
      { modMask            = mod4Mask
      , borderWidth        = myBorderWidth
      , normalBorderColor  = myNormalBorderColor
      , focusedBorderColor = myFocusedBorderColor
      , layoutHook         = myLayoutHook
      , manageHook         = namedScratchpadManageHook scratchpads <+> fullscreenManageHook <+> manageDocks
      , handleEventHook    = fullscreenEventHook
      , logHook            = myLogHook wsBar
      } `additionalKeysP` myKeys
-- }}}
-- Layout ----------------------------------------------------------------- {{{
myLayoutHook = avoidStruts
             $ smartBorders
             $ fullscreenFull
             $ mkToggle (FULL ?? EOT)
             $ mkToggle (single REFLECTX)
             $ mkToggle (single REFLECTY)
             $ mkToggle (single MIRROR)
             $ mkToggle (single NOBORDERS)
             $ lMas ||| lGrd ||| lTal
               where
                 gap  = 4
                 spc  = spacingRaw True (Border gap gap gap gap) True (Border gap gap gap gap) True
                 inc  = 1/100
                 asp  = 16/9
                 grto = toRational $ 2/(1 + sqrt 5)
                 lMas = named "Master"   $ spc $ Tall 1 inc grto
                 lGrd = named "Grid"     $ spc $ GridRatio asp
                 lTal = named "Tall"     $ spc $ Mirror $ Tall 0 inc grto
-- }}}
-- Scratchpads ------------------------------------------------------------ {{{
scratchpads = [ NS "terminal" spawnTerm findTerm manageTerm ]
  where
    spawnTerm  = "$XDG_CONFIG_HOME/xmonad/scripts/choose-term.sh --class scratchpad"
    findTerm   = resource =? "scratchpad"
    manageTerm = customFloating $ W.RationalRect l t w h
      where
        h = 0.9
        w = 0.9
        t = 0.95 - h
        l = 0.95 - w
-- }}}
-- Key Bindings ----------------------------------------------------------- {{{
dmenuArgs = "-fn '" ++ myDmenuFont ++ "' -nb '" ++ myDmenuNormBG ++ "' -sb '" ++ myDmenuSelBG ++ "' -nf '" ++ myDmenuNormFG ++ "' -sf '" ++ myDmenuSelFG ++ "'"
myKeys =
  [
    -- Launcher
    ("M-p",         spawn "rofi -show drun")
    -- Xmonad
  , ("M-r",         spawn "if type xmonad; then xmonad --recompile && xmonad --restart; else xmessage xmonad not in \\$PATH: \"$PATH\"; fi")
    -- Windows
  , ("M-q",         kill)
  , ("M-s",         namedScratchpadAction scratchpads "terminal")
  -- Windows
  , ("M-<End>",     spawn "$XDG_CONFIG_HOME/lock.sh")
  , ("M-S-s",       spawn "flameshot gui --clipboard --accept-on-select")
    -- Layout
  , ("M-f",         sendMessage $ Toggle FULL)
  , ("M-x",         sendMessage $ Toggle REFLECTX)
  , ("M-y",         sendMessage $ Toggle REFLECTY)
  , ("M-z",         sendMessage $ Toggle MIRROR)
    -- Workspaces
  , ("M-<Tab>",     moveTo Next (Not emptyWS))
  , ("M-S-<Tab>",   moveTo Prev (Not emptyWS))
    -- Programs
  , ("M-<Return>",  spawn "$XDG_CONFIG_HOME/xmonad/scripts/choose-term.sh")
  , ("M-o",         spawn "$XDG_CONFIG_HOME/xmonad/scripts/choose-browser.sh")
  , ("M-i",         spawn "$XDG_CONFIG_HOME/xmonad/scripts/choose-editor.sh")
  , ("M-S-o",       spawn ("item=$(echo 'Prime Video\\nChatGPT\\nLichess\\nNetflix\\nYouTube' | rofi -dmenu -i -no-show-icons -p 'Chromium App') && $XDG_CONFIG_HOME/xmonad/scripts/choose-browser.sh $item"))
    -- Media
  , ("M-m",         spawn "$XDG_CONFIG_HOME/xmonad/scripts/mute.sh")
    -- Appearance
  , ("M-w",         spawn "$XDG_CONFIG_HOME/wallpaper/wallpaper.sh")
  ]
-- }}}
-- Xmobar ----------------------------------------------------------------- {{{
myLogHook h = dynamicLogWithPP (wsPP { ppOutput = hPutStrLn h }) >> atomHook
myWsBar     = "xmobar $XDG_CONFIG_HOME/xmonad/xmobar.hs"
wsPP        = xmobarPP
              { ppOrder   = \(ws:l:t:r) -> ws:l:t:r
              , ppTitle   = shorten 64
              , ppCurrent = \_ -> wrap "<fn=2>" "</fn>" "\xf111"
              , ppHidden  = \_ -> wrap "<fn=1>" "</fn>" "\xf111"
              , ppLayout  = \x -> if "Full" `isInfixOf` x
                                    then "<fn=2><fc=#ff5555>\xf2d0</fc></fn>"
                                    else case x of
                                      "Master" -> "<fn=2>\xf0c9</fn>"
                                      "Tall"   -> "<fn=2>\xf0db</fn>"
                                      "Grid"   -> "<fn=2>\xf00a</fn>"
                                      _        -> x
              , ppSep     = "   "
              , ppWsSep   = " "
              }
-- }}}
-- Atom Hook -------------------------------------------------------------- {{{
atomHook :: X ()
atomHook = do
  ws <- gets windowset
  let allWins = W.index ws
      floats  = W.floating ws
      tiledWins = filter (`M.notMember` floats) allWins
  withDisplay $ \dpy -> do
    atom <- getAtom "_NET_WM_STATE_SINGLE"
    card <- getAtom "CARDINAL"
    case tiledWins of
      [w] -> do
        io $ changeProperty32 dpy w atom card 0 [1]
        mapM_ (\o -> when (o /= w) $ io $ deleteProperty dpy o atom) allWins
      _   -> mapM_ (\w -> io $ deleteProperty dpy w atom) allWins
-- }}}
