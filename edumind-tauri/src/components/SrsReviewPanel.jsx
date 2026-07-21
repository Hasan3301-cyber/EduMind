import { useEffect, useState } from "react";

import { StudyInsightsPanel } from "./StudyInsightsPanel";

const REVIEW_GRADES = [
  { rating: 0, label: "Again", description: "Reset the learning step" },
  { rating: 3, label: "Good", description: "Continue the normal interval" },
  { rating: 5, label: "Easy", description: "Increase the interval faster" }
];

export function SrsReviewPanel({ client }) {
  const [cards, setCards] = useState([]);
  const [previews, setPreviews] = useState({});
  const [reviewingCardId, setReviewingCardId] = useState(null);
  const [status, setStatus] = useState("Loading due cards…");

  async function refresh() {
    if (!client) {
      setCards([]);
      setPreviews({});
      setStatus("Launch the desktop app to load local SRS cards.");
      return;
    }
    try {
      const due = await client.dueSrsCards({ limit: 20 });
      const nextCards = Array.isArray(due) ? due : [];
      setCards(nextCards);
      if (!nextCards.length) {
        setPreviews({});
        setStatus("No cards are due right now.");
        return;
      }

      const previewRequests = nextCards.flatMap((card) =>
        REVIEW_GRADES.map(async ({ rating }) => ({
          cardId: card.id,
          rating,
          preview: await client.previewSrsReview(card.id, rating)
        }))
      );
      const settled = await Promise.allSettled(previewRequests);
      const nextPreviews = {};
      let unavailable = 0;
      for (const result of settled) {
        if (result.status !== "fulfilled") {
          unavailable += 1;
          continue;
        }
        const { cardId, rating, preview } = result.value;
        nextPreviews[previewKey(cardId, rating)] = preview;
      }
      setPreviews(nextPreviews);
      setStatus(
        unavailable
          ? `${nextCards.length} cards are ready, but ${unavailable} grade ${unavailable === 1 ? "preview is" : "previews are"} unavailable. Refresh before grading.`
          : `${nextCards.length} cards are ready. Every grade shows its exact next-review consequence.`
      );
    } catch (error) {
      setCards([]);
      setPreviews({});
      setStatus(error.message);
    }
  }

  useEffect(() => {
    void refresh();
  }, [client]);

  async function review(card, rating) {
    if (!client || reviewingCardId || !previews[previewKey(card.id, rating)]) {
      return;
    }
    setReviewingCardId(card.id);
    try {
      await client.reviewSrsCard(card.id, rating);
      setStatus("Grade saved to local review history. Loading the next due card…");
      await refresh();
    } catch (error) {
      setStatus(error.message);
    } finally {
      setReviewingCardId(null);
    }
  }

  return (
    <section className="srs-panel">
      <StudyInsightsPanel client={client} />
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Spaced repetition</p>
          <h1>Review what is due, then return to deep work.</h1>
        </div>
        <button type="button" className="secondary-button" onClick={() => void refresh()} disabled={Boolean(reviewingCardId)}>Refresh</button>
      </div>
      <p className="muted" aria-live="polite">{status}</p>
      <div className="srs-card-list">
        {cards.map((card) => (
          <article className="srs-card" key={card.id}>
            <p className="eyebrow">{card.deck ?? "Default deck"}</p>
            <h2>{card.front}</h2>
            <p>{card.back}</p>
            <p className="review-preview-disclosure">Choose only after checking the consequence below. No grade is saved until you press a grade button.</p>
            <div className="review-actions" aria-label={`Review ${card.front}`}>
              {REVIEW_GRADES.map((grade) => {
                const preview = previews[previewKey(card.id, grade.rating)];
                const consequence = preview ? formatPreview(preview) : "Preview unavailable";
                return (
                  <button
                    type="button"
                    key={grade.rating}
                    onClick={() => void review(card, grade.rating)}
                    disabled={Boolean(reviewingCardId) || !preview}
                    aria-label={`${grade.label}: ${consequence}`}
                  >
                    <strong>{grade.label}</strong>
                    <span>{grade.description}</span>
                    <small>{consequence}</small>
                  </button>
                );
              })}
            </div>
          </article>
        ))}
      </div>
    </section>
  );
}

function previewKey(cardId, rating) {
  return `${cardId}:${rating}`;
}

function formatPreview(preview) {
  const days = Number(preview?.interval_days ?? 0);
  const dueAt = new Date(preview?.due_at);
  const dateLabel = Number.isNaN(dueAt.getTime())
    ? "date unavailable"
    : new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" }).format(dueAt);
  return `Next review in ${days} ${days === 1 ? "day" : "days"} · ${dateLabel}`;
}
