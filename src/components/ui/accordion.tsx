import * as AccordionPrimitive from '@radix-ui/react-accordion';
import { forwardRef, type ComponentPropsWithoutRef, type ElementRef } from 'react';
import { cn } from '../../lib/utils';

/**
 * Square UI grammar accordion on the Radix Accordion primitive: keyboard
 * behaviour, aria-expanded wiring and height animation state come from
 * Radix; surfaces are styled through the token variables in styles.css.
 */
export const Accordion = AccordionPrimitive.Root;

export const AccordionItem = forwardRef<
  ElementRef<typeof AccordionPrimitive.Item>,
  ComponentPropsWithoutRef<typeof AccordionPrimitive.Item>
>(({ className, ...props }, ref) => (
  <AccordionPrimitive.Item ref={ref} className={cn('ui-accordion-item', className)} {...props} />
));
AccordionItem.displayName = 'AccordionItem';

export const AccordionTrigger = forwardRef<
  ElementRef<typeof AccordionPrimitive.Trigger>,
  ComponentPropsWithoutRef<typeof AccordionPrimitive.Trigger>
>(({ className, ...props }, ref) => (
  <AccordionPrimitive.Header style={{ margin: 0, font: 'inherit' }}>
    <AccordionPrimitive.Trigger ref={ref} className={cn('ui-accordion-trigger', className)} {...props} />
  </AccordionPrimitive.Header>
));
AccordionTrigger.displayName = 'AccordionTrigger';

export const AccordionContent = forwardRef<
  ElementRef<typeof AccordionPrimitive.Content>,
  ComponentPropsWithoutRef<typeof AccordionPrimitive.Content>
>(({ className, ...props }, ref) => (
  <AccordionPrimitive.Content ref={ref} className={cn('ui-accordion-content', className)} {...props} />
));
AccordionContent.displayName = 'AccordionContent';
