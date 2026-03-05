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
import XMonad.Util.EZConfig
import XMonad.Util.NamedScratchpad
import XMonad.Util.Run
import XMonad.Util.WorkspaceCompare (getSortByIndex)
import qualified XMonad.StackSet as W
import qualified XMonad.Util.ExtensibleState as XS
import qualified Data.Map as M
import qualified Data.Set as S
import Control.Monad (forM_, unless, when)
import Graphics.X11.Xlib.Extras
import Data.List (isInfixOf)
import Data.Maybe (fromMaybe)
import System.IO (Handle)
-- }}}
-- Startup ---------------------------------------------------------------- {{{
myStartupHook :: X ()
myStartupHook = do
  spawn "autocutsel -fork -selection CLIPBOARD"
  spawn "autocutsel -fork -selection PRIMARY"
  refresh
-- }}}
-- Theme ------------------------------------------------------------------ {{{
myBorderWidth :: Dimension
myBorderWidth        = 3
myNormalBorderColor :: String
myNormalBorderColor  = "#1a1a1a"
myFocusedBorderColor :: String
myFocusedBorderColor = "#61afef"
myPinnedBorderColor :: String
myPinnedBorderColor  = "#b48ead"
-- }}}
-- Main ------------------------------------------------------------------- {{{
main :: IO ()
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
      , manageHook         = namedScratchpadManageHook scratchpads <+> fullscreenManageHook
      , handleEventHook    = fullscreenEventHook
      , startupHook        = myStartupHook
      , logHook            = myLogHook wsBar
      } `additionalKeysP` myKeys
-- }}}
-- Layout ----------------------------------------------------------------- {{{
myLayoutHook = avoidStruts
             $ smartBorders
             $ mkToggle (NOBORDERS ?? EOT)
             $ mkToggle (FULL ?? EOT)
             $ fullscreenFull
             $ mkToggle (single REFLECTX)
             $ mkToggle (single REFLECTY)
             $ mkToggle (single MIRROR)
             $ master ||| grid ||| tall
               where
                 gap    = 4
                 gaps   = spacingRaw True (Border gap gap gap gap) True (Border gap gap gap gap) True
                 delta  = 1/100
                 aspect = 16/9
                 ratio  = 3/4
                 master = named "Master" $ gaps $ Tall 1 delta ratio
                 grid   = named "Grid"   $ gaps $ GridRatio aspect
                 tall   = named "Tall"   $ gaps $ Mirror $ Tall 0 delta ratio
-- }}}
-- Scratchpads ------------------------------------------------------------ {{{
scratchpads :: [NamedScratchpad]
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
myKeys :: [(String, X ())]
myKeys =
  [
    -- Launcher
    ("M-p",         spawn "rofi -show drun")
    -- Xmonad
  , ("M-r",         spawn "if type xmonad; then xmonad --recompile && xmonad --restart; else xmessage xmonad not in \\$PATH: \"$PATH\"; fi")
    -- Windows
  , ("M-q",         withFocused unpin >> kill)
  , ("M-s",         namedScratchpadAction scratchpads "terminal")
  -- System
  , ("M-<End>",     spawn "$XDG_CONFIG_HOME/lock.sh")
  , ("M-S-s",       spawn "flameshot gui --clipboard --accept-on-select")
    -- Layout
  , ("M-f",         sendMessage $ Toggle FULL)
  , ("M-b",         sendMessage ToggleStruts)
  , ("M-x",         sendMessage $ Toggle REFLECTX)
  , ("M-y",         sendMessage $ Toggle REFLECTY)
  , ("M-z",         sendMessage $ Toggle MIRROR)
    -- Workspaces
  , ("M-<Tab>",     findWorkspace getSortByIndex Next (Not emptyWS) 1 >>= switchWorkspace)
  , ("M-S-<Tab>",   findWorkspace getSortByIndex Prev (Not emptyWS) 1 >>= switchWorkspace)
    -- Programs
  , ("M-<Return>",  spawn "$XDG_CONFIG_HOME/xmonad/scripts/choose-term.sh")
  , ("M-o",         spawn "$XDG_CONFIG_HOME/xmonad/scripts/choose-browser.sh")
  , ("M-i",         spawn "$XDG_CONFIG_HOME/xmonad/scripts/choose-editor.sh")
  , ("M-S-o",       spawn ("item=$(echo 'Prime Video\\nChatGPT\\nLichess\\nNetflix\\nYouTube' | rofi -dmenu -i -no-show-icons -p 'Chromium App') && $XDG_CONFIG_HOME/xmonad/scripts/choose-browser.sh $item"))
    -- Media
  , ("M-m",         spawn "$XDG_CONFIG_HOME/xmonad/scripts/mute.sh")
    -- Appearance
  , ("M-w",         spawn "$XDG_CONFIG_HOME/wallpaper/wallpaper.sh")
    -- Pin window to follow across workspaces
  , ("M-v",         togglePin)
  ]
  ++ [("M-" ++ show n, switchWorkspace (show n)) | n <- [1..9 :: Int]]
-- }}}
-- Xmobar ----------------------------------------------------------------- {{{
myLogHook :: Handle -> X ()
myLogHook h = do
  dynamicLogWithPP (wsPP { ppOutput = hPutStrLn h })
  pinLogHook
  pinBorderHook
  atomHook

myWsBar :: String
myWsBar = "xmobar $XDG_CONFIG_HOME/xmonad/xmobar.hs"

wsPP :: PP
wsPP = xmobarPP
  { ppTitle   = shorten 64
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
-- Pinned Windows --------------------------------------------------------- {{{

-- | Switch workspace and move pinned windows in a single atomic operation
-- to avoid a double-render (which causes visible resize jumps).
switchWorkspace :: WorkspaceId -> X ()
switchWorkspace ws = do
  PinnedWindows pinned <- XS.get
  windows $ movePins pinned . W.greedyView ws

movePins :: (Eq i, Eq s) => S.Set Window -> W.StackSet i l Window s sd -> W.StackSet i l Window s sd
movePins pinned ss
  | S.null toMove = ss
  | otherwise     = sinkPinnedToEnd pinned shifted
  where
    cur     = W.currentTag ss
    curSet  = S.fromList (W.index ss)
    toMove  = S.difference pinned curSet
    shifted = S.foldl' (\acc w -> W.shiftWin cur w acc) ss toMove

newtype PinnedWindows = PinnedWindows (S.Set Window)

instance ExtensionClass PinnedWindows where
  initialValue = PinnedWindows S.empty

togglePin :: X ()
togglePin = withFocused $ \w ->
  XS.modify $ \(PinnedWindows s) ->
    PinnedWindows $ if S.member w s then S.delete w s else S.insert w s

unpin :: Window -> X ()
unpin w = XS.modify $ \(PinnedWindows s) -> PinnedWindows (S.delete w s)

pinBorderHook :: X ()
pinBorderHook = do
  PinnedWindows pinned <- XS.get
  ws <- gets windowset
  let focused = W.peek ws
  withDisplay $ \dpy -> do
    pinnedPixel <- fromMaybe 0 <$> io (initColor dpy myPinnedBorderColor)
    normalPixel <- fromMaybe 0 <$> io (initColor dpy myNormalBorderColor)
    focusPixel  <- fromMaybe 0 <$> io (initColor dpy myFocusedBorderColor)
    forM_ (W.index ws) $ \w ->
      let color
            | Just w == focused = focusPixel
            | S.member w pinned = pinnedPixel
            | otherwise         = normalPixel
      in io $ setWindowBorder dpy w color

pinLogHook :: X ()
pinLogHook = do
  PinnedWindows pinned <- XS.get
  unless (S.null pinned) $ do
    ws <- gets windowset
    let cur    = W.currentTag ws
        curSet = S.fromList (W.index ws)
        toMove = S.difference pinned curSet
    unless (S.null toMove) $
      windows $ \s ->
        let shifted = S.foldl' (\acc w -> W.shiftWin cur w acc) s toMove
        in sinkPinnedToEnd pinned shifted

sinkPinnedToEnd :: S.Set Window -> W.StackSet i l Window s sd -> W.StackSet i l Window s sd
sinkPinnedToEnd pinned ss =
  case W.stack . W.workspace . W.current $ ss of
    Nothing  -> ss
    Just stk ->
      let stk' = pushToEnd pinned stk
          cur   = W.current ss
          ws    = W.workspace cur
      in ss { W.current = cur { W.workspace = ws { W.stack = Just stk' } } }

pushToEnd :: S.Set Window -> W.Stack Window -> W.Stack Window
pushToEnd pinned (W.Stack f u d)
  | S.member f pinned =
      let allOrdered = reverse u ++ [f] ++ d
          normal = filter (`S.notMember` pinned) allOrdered
          pins   = filter (`S.member` pinned) allOrdered
      in case normal of
           []     -> W.Stack f u d
           (n:ns) -> W.Stack n [] (ns ++ pins)
  | otherwise =
      let normalBef = filter (`S.notMember` pinned) (reverse u)
          normalAft = filter (`S.notMember` pinned) d
          pins      = filter (`S.member` pinned) (reverse u ++ d)
      in W.Stack f (reverse normalBef) (normalAft ++ pins)
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
