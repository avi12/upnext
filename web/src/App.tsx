import { Suspense, lazy, useEffect, useState, type ReactNode } from 'react'
import { BrowserRouter, Navigate, Outlet, Route, Routes, useLocation } from 'react-router'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { Shell } from './components/Shell'
import { ToastProvider } from './components/Toast'
import { PickPage } from './pages/PickPage'
import { LibraryPage } from './pages/LibraryPage'
import { PairPage } from './pages/PairPage'
import { tellWindow, useBigScreen } from './lib/bigscreen'
import { BigScreenOffer } from './components/bigscreen/BigScreenOffer'
import { ApprovalDialog } from './components/ApprovalDialog'
import { PlayerGate } from './components/players/WhoIsPlaying'
import { ErrorBoundary } from './components/ErrorBoundary'
import { keys, useSetupState } from './lib/queries'

// The other pages load the first time they're opened, which keeps the first
// load to Pick and Library.
const VersusPage = lazy(() => import('./pages/VersusPage').then((m) => ({ default: m.VersusPage })))
const CollectionsPage = lazy(() => import('./pages/CollectionsPage').then((m) => ({ default: m.CollectionsPage })))
const HistoryPage = lazy(() => import('./pages/HistoryPage').then((m) => ({ default: m.HistoryPage })))
const ListsPage = lazy(() => import('./pages/ListsPage').then((m) => ({ default: m.ListsPage })))
const SettingsPage = lazy(() => import('./pages/SettingsPage').then((m) => ({ default: m.SettingsPage })))
const ProfilePage = lazy(() => import('./pages/ProfilePage').then((m) => ({ default: m.ProfilePage })))
const QuestionsPage = lazy(() => import('./pages/QuestionsPage').then((m) => ({ default: m.QuestionsPage })))
const SetupPage = lazy(() => import('./pages/SetupPage').then((m) => ({ default: m.SetupPage })))
const PlayerSetupPage = lazy(() => import('./pages/PlayerSetupPage').then((m) => ({ default: m.PlayerSetupPage })))
const ConnectPage = lazy(() => import('./pages/ConnectPage').then((m) => ({ default: m.ConnectPage })))
const GogCallbackPage = lazy(() => import('./pages/GogCallbackPage').then((m) => ({ default: m.GogCallbackPage })))
// Big screen loads only when it's turned on.
const BigShell = lazy(() => import('./components/bigscreen/BigShell').then((m) => ({ default: m.BigShell })))
const BigCuratePage = lazy(() => import('./pages/big/BigCuratePage').then((m) => ({ default: m.BigCuratePage })))
const BigGamePage = lazy(() => import('./pages/big/BigGamePage').then((m) => ({ default: m.BigGamePage })))
const BigLibraryPage = lazy(() => import('./pages/big/BigLibraryPage').then((m) => ({ default: m.BigLibraryPage })))
const BigListsPage = lazy(() => import('./pages/big/BigListsPage').then((m) => ({ default: m.BigListsPage })))
const BigDecidePage = lazy(() => import('./pages/big/BigDecidePage').then((m) => ({ default: m.BigDecidePage })))
const BigSettingsPage = lazy(() => import('./pages/big/BigSettingsPage').then((m) => ({ default: m.BigSettingsPage })))
const BigSetupPage = lazy(() => import('./pages/big/BigSetupPage').then((m) => ({ default: m.BigSetupPage })))
const PointerOnlyPage = lazy(() => import('./pages/big/PointerOnlyPage').then((m) => ({ default: m.PointerOnlyPage })))

/** A page that failed shows what went wrong in its place; opening another
 *  page clears it. */
function PageErrors({ children }: { children: ReactNode }) {
  const { pathname } = useLocation()
  return <ErrorBoundary resetKey={pathname}>{children}</ErrorBoundary>
}

/** A page that loads when first opened: nothing shows in its place meanwhile. */
function Later({ children }: { children: ReactNode }) {
  return <Suspense fallback={null}>{children}</Suspense>
}

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      retry: 1,
      refetchOnWindowFocus: false,
    },
  },
})

// The desktop app (`upnext app`) says when its window gets the focus back:
// a Steam sign-in may have finished in the browser meanwhile.
window.addEventListener('upnext:focus', () => {
  for (const key of [keys.profile, keys.settings, keys.config]) queryClient.invalidateQueries({ queryKey: key })
})

/** A first run (an empty library, setup not finished or skipped) goes to
 *  the setup wizard from every page, and a new player to their own short
 *  setup, except in big screen, which has no keyboard for it. */
function SetupGate() {
  const big = useBigScreen()
  const state = useSetupState()
  if (state.data?.needed) return <Navigate to="/setup" replace />
  if (state.data?.player_needed && !big) return <Navigate to="/setup/player" replace />
  return <Outlet />
}

/** Big screen's pages at the same addresses as the normal ones, so turning
 *  it on or off keeps the page; pages made for a pointer offer to leave it. */
function bigRoutes() {
  return (
    <Route element={<Later><BigShell /></Later>}>
      <Route index element={<Later><BigCuratePage /></Later>} />
      <Route path="game/:id" element={<Later><BigGamePage /></Later>} />
      <Route path="library" element={<Later><BigLibraryPage /></Later>} />
      <Route path="lists" element={<Later><BigListsPage /></Later>} />
      <Route path="versus" element={<Later><BigDecidePage /></Later>} />
      <Route path="settings" element={<Later><BigSettingsPage /></Later>} />
      <Route path="history" element={<Later><PointerOnlyPage name="History" /></Later>} />
      <Route path="collections" element={<Later><PointerOnlyPage name="Collections" /></Later>} />
      <Route path="profile/*" element={<Later><PointerOnlyPage name="Your taste profile" /></Later>} />
      <Route path="later" element={<Navigate to="/lists" replace />} />
      <Route path="*" element={<Navigate to="/" replace />} />
    </Route>
  )
}

export default function App() {
  const big = useBigScreen()
  // The app window goes full screen with big screen, and back after.
  useEffect(() => tellWindow(big), [big])
  // Another device without a pairing gets 401 from the API: ask for the code.
  const [unpaired, setUnpaired] = useState(false)
  useEffect(() => {
    const ask = () => setUnpaired(true)
    window.addEventListener('upnext:unpaired', ask)
    return () => window.removeEventListener('upnext:unpaired', ask)
  }, [])
  if (unpaired) {
    return (
      <QueryClientProvider client={queryClient}>
        <PairPage
          onPaired={() => {
            setUnpaired(false)
            queryClient.invalidateQueries()
          }}
        />
      </QueryClientProvider>
    )
  }
  return (
    <QueryClientProvider client={queryClient}>
      <ToastProvider>
        <BrowserRouter>
          <PageErrors>
            <Routes>
              {big ? (
                // The first run with a controller: big screen's
                // input, without its pages.
                <Route element={<Later><BigShell bare /></Later>}>
                  <Route path="setup" element={<Later><BigSetupPage /></Later>} />
                  <Route path="connect" element={<Later><ConnectPage /></Later>} />
                </Route>
              ) : (
                <>
                  <Route path="setup" element={<Later><SetupPage /></Later>} />
                  <Route path="setup/player" element={<Later><PlayerSetupPage /></Later>} />
                  {/* Another computer's library in the app window. */}
                  <Route path="connect" element={<Later><ConnectPage /></Later>} />
                </>
              )}
              <Route path="gog/callback" element={<Later><GogCallbackPage /></Later>} />
              <Route element={<SetupGate />}>
                {big ? bigRoutes() : (
                <Route element={<Shell />}>
                  <Route index element={<PickPage />} />
                  <Route path="versus" element={<Later><VersusPage /></Later>} />
                  <Route path="library" element={<LibraryPage />} />
                  <Route path="collections" element={<Later><CollectionsPage /></Later>} />
                  <Route path="history" element={<Later><HistoryPage /></Later>} />
                  <Route path="lists" element={<Later><ListsPage /></Later>} />
                  <Route path="settings" element={<Later><SettingsPage /></Later>} />
                  <Route path="profile" element={<Later><ProfilePage /></Later>} />
                  <Route path="profile/questions" element={<Later><QuestionsPage /></Later>} />
                  {/* Old addresses. */}
                  <Route path="collection" element={<Navigate to="/library?view=table" replace />} />
                  <Route path="data" element={<Navigate to="/settings#data" replace />} />
                  <Route path="later" element={<Navigate to="/lists" replace />} />
                  <Route path="*" element={<Navigate to="/" replace />} />
                </Route>
                )}
              </Route>
            </Routes>
          </PageErrors>
          {!big && <BigScreenOffer />}
          {/* A device asking to be allowed, on the computer UpNext runs on. */}
          <ApprovalDialog />
          {/* "Who's playing?" with two or more players. */}
          <PlayerGate />
        </BrowserRouter>
      </ToastProvider>
    </QueryClientProvider>
  )
}
