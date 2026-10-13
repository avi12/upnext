import { useNavigate } from 'react-router'
import { TasteQuestions } from '../components/questions/TasteQuestions'

/** "Tune your profile": the first-run questions as a page of their own.
 *  The setup wizard shows them as a step too. */
export function QuestionsPage() {
  const navigate = useNavigate()
  return (
    <div className="mx-auto flex max-w-[1100px] flex-col gap-6 px-4 pt-8 pb-24 sm:px-10">
      <p className="m-0 max-w-[760px] text-[17px] leading-[1.6] text-muted">
        <span className="text-ink">Tune your profile.</span> A couple of minutes, and the first picks fit you better, especially when you ask for
        “anything”. Every answer saves as you give it, on this computer, and a short summary of your profile goes to your AI with each pick.
      </p>
      <TasteQuestions onFinish={(to) => navigate(to === 'pick' ? '/' : '/profile')} />
    </div>
  )
}
