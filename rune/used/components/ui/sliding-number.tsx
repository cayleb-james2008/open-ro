'use client';
import { useEffect, useId } from 'react';
import {
  MotionValue,
  motion,
  useReducedMotion,
  useSpring,
  useTransform,
  motionValue,
} from 'motion/react';
import useMeasure from 'react-use-measure';

const TRANSITION = {
  type: 'spring',
  stiffness: 280,
  damping: 18,
  mass: 0.3,
} as const;

function Digit({ value, place }: { value: number; place: number }) {
  const valueRoundedToPlace = Math.floor(value / place) % 10;
  const initial = motionValue(valueRoundedToPlace);
  const animatedValue = useSpring(initial, TRANSITION);
  const reduce = useReducedMotion(); // local edit: a spring on a MotionValue ignores MotionConfig, so the roll is skipped here

  useEffect(() => {
    if (reduce) animatedValue.jump(valueRoundedToPlace);
    else animatedValue.set(valueRoundedToPlace);
  }, [animatedValue, valueRoundedToPlace, reduce]);

  return (
    <div className='relative inline-block w-[1ch] overflow-x-visible overflow-y-clip leading-none tabular-nums'>
      <div className='invisible'>0</div>
      {Array.from({ length: 10 }, (_, i) => (
        <Number key={i} mv={animatedValue} number={i} />
      ))}
    </div>
  );
}

function Number({ mv, number }: { mv: MotionValue<number>; number: number }) {
  const uniqueId = useId();
  const [ref, bounds] = useMeasure();

  const y = useTransform(mv, (latest) => {
    if (!bounds.height) return 0;
    const placeValue = latest % 10;
    const offset = (10 + number - placeValue) % 10;
    let memo = offset * bounds.height;

    if (offset > 5) {
      memo -= 10 * bounds.height;
    }

    return memo;
  });

  // don't render the animated number until we know the height
  if (!bounds.height) {
    return (
      <span ref={ref} className='invisible absolute'>
        {number}
      </span>
    );
  }

  return (
    <motion.span
      style={{ y }}
      layoutId={`${uniqueId}-${number}`}
      className='absolute inset-0 flex items-center justify-center'
      transition={TRANSITION}
      ref={ref}
    >
      {number}
    </motion.span>
  );
}

type SlidingNumberProps = {
  value: number;
  padStart?: boolean;
  decimalSeparator?: string;
  /** local edit: a thousands separator between groups of three digits, for money and big counts */
  group?: string;
};

export function SlidingNumber({
  value,
  padStart = false,
  decimalSeparator = '.',
  group,
}: SlidingNumberProps) {
  const absValue = Math.abs(value);
  const [integerPart, decimalPart] = absValue.toString().split('.');
  const integerValue = parseInt(integerPart, 10);
  const paddedInteger =
    padStart && integerValue < 10 ? `0${integerPart}` : integerPart;
  const integerDigits = paddedInteger.split('');
  const integerPlaces = integerDigits.map((_, i) =>
    Math.pow(10, integerDigits.length - i - 1)
  );

  // local edit: the rolling columns hold every digit 0 to 9, so they are hidden from assistive technology
  // and the real value is exposed once, as text.
  const groupedInteger = group ? paddedInteger.replace(/\B(?=(\d{3})+(?!\d))/g, group) : paddedInteger;
  const spoken = `${value < 0 ? '-' : ''}${groupedInteger}${decimalPart ? decimalSeparator + decimalPart : ''}`;

  return (
    <div className='flex items-center'>
      <span className='sr-only'>{spoken}</span>
      <div aria-hidden='true' className='flex items-center'>
        {value < 0 && '-'}
        {integerDigits.map((_, index) => (
          <span key={`pos-${integerPlaces[index]}`} className='flex items-center'>
            {group && index > 0 && (integerDigits.length - index) % 3 === 0 && <span className='-mx-[0.2ch]'>{group}</span>}
            <Digit value={integerValue} place={integerPlaces[index]} />
          </span>
        ))}
        {decimalPart && (
          <>
            <span>{decimalSeparator}</span>
            {decimalPart.split('').map((_, index) => (
              <Digit
                key={`decimal-${index}`}
                value={parseInt(decimalPart, 10)}
                place={Math.pow(10, decimalPart.length - index - 1)}
              />
            ))}
          </>
        )}
      </div>
    </div>
  );
}
