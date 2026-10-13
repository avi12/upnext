import { costText, pricesTitle, useShowUsd } from '../lib/money'
import { useAiPrices } from '../lib/useAiPrices'

/**
 * What some AI calls cost, in US dollars, when "Show costs in US dollars" is on; nothing otherwise.
 * Hovering it gives the prices' date; as a link (`link`) it opens the provider's pricing page.
 */
export function Money({ cost, link = false, prefix = '' }: { cost: { cost_usd?: number | null; api_usd?: number | null }; link?: boolean; prefix?: string }) {
  const show = useShowUsd()
  const prices = useAiPrices()
  const text = show ? costText({ cost_usd: cost.cost_usd ?? null, api_usd: cost.api_usd ?? null }) : null
  if (!text) return null
  const info = prices.data?.prices
  const title = pricesTitle(info)
  return (
    <>
      {prefix}
      {link && info ? (
        <a href={info.page_url} target="_blank" rel="noreferrer" title={title} className="underline decoration-underline underline-offset-2 hover:text-ink">
          {text}
        </a>
      ) : (
        <span title={title}>{text}</span>
      )}
    </>
  )
}
