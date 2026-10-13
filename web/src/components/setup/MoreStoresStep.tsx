import { ByHandRow } from '../settings/ByHand'
import { EpicRow } from '../settings/Epic'
import { plural } from '../../lib/format'
import { RowGrid } from '../settings/parts'
import { StepButtons, StepHead, type StepProps } from './parts'

/** After GOG's step (4a, so the other steps keep their numbers): the Epic
 *  Games Launcher, the EA app and Ubisoft Connect, read for themselves
 *  rather than through Heroic or GOG Galaxy, and the stores whose
 *  games can only be listed by hand. */
export function MoreStoresStep({ setup, next, back }: StepProps) {
  const epic = setup.stores.epic
  const installed = setup.stores.ea.installed + setup.stores.ubisoft.installed
  return (
    <section aria-labelledby="step-more-stores" className="flex flex-col gap-6">
      <StepHead id="step-more-stores" title="Epic, EA and Ubisoft">
        <p className="m-0">
          {setup.stores.heroic.path ? 'Heroic brings your Epic games. ' : ''}Signing in to Epic brings every Epic game you own, with
          its playtime, Heroic or not.{' '}
          {epic.installed != null && <>The Epic Games Launcher is here, with {plural(epic.installed, 'game')} installed.</>}
        </p>
        <p className="m-0">
          {setup.environment.os === 'windows' && (
            <>
              The games the EA app and Ubisoft Connect installed on this computer come in by themselves
              {installed ? ` (${plural(installed, 'game')} now)` : ''}.{' '}
            </>
          )}
          The EA app, Ubisoft Connect, Battle.net and Xbox have no sign-in UpNext can use, so their libraries can't be read.
          GOG Galaxy reads them where it's linked to them; otherwise list your games from them by hand, and they're picked like
          any others.
        </p>
        <p className="m-0">Next starts the first sync in the background, so it reads your libraries while you finish setting up.</p>
      </StepHead>
      <RowGrid>
        <EpicRow launcherDir={epic.launcher_dir} />
        <ByHandRow />
      </RowGrid>
      <StepButtons onNext={next} onBack={back} onSkip={next} />
    </section>
  )
}
