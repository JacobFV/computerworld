export type ColumnId = 'todo' | 'progress' | 'review' | 'done';
export type Priority = 'Low' | 'Medium' | 'High';

export type Card = {
  id: number;
  title: string;
  column: ColumnId;
  priority: Priority;
  labels: string[];
  assignees: string[];
  due?: string;
  comments: number;
  files: number;
  progress?: number;
};

export const people: Record<string, { name: string; initials: string; gradient: string }> = {
  ar: { name: 'Ava Reyes', initials: 'AR', gradient: 'from-pink-500 to-orange-400' },
  jk: { name: 'Jun Kato', initials: 'JK', gradient: 'from-sky-500 to-indigo-500' },
  ml: { name: 'Mia Lopez', initials: 'ML', gradient: 'from-emerald-500 to-teal-400' },
  do: { name: 'Dev Okafor', initials: 'DO', gradient: 'from-violet-500 to-fuchsia-500' },
};

export const initialCards: Card[] = [
  { id: 1, title: 'Audit the pricing page copy', column: 'todo', priority: 'Low', labels: ['Research'], assignees: ['ml'], due: 'May 3', comments: 2, files: 0 },
  { id: 2, title: 'Hero illustration for the relaunch', column: 'todo', priority: 'Medium', labels: ['Design'], assignees: ['ar', 'do'], due: 'May 6', comments: 5, files: 3 },
  { id: 3, title: 'Checkout crashes on Safari 16 when the coupon field is empty', column: 'todo', priority: 'High', labels: ['Bug', 'Frontend'], assignees: ['jk'], comments: 8, files: 1 },
  { id: 4, title: 'Migrate the blog to the new CMS', column: 'progress', priority: 'Medium', labels: ['Backend'], assignees: ['do'], due: 'May 9', comments: 1, files: 0, progress: 60 },
  { id: 5, title: 'Responsive navigation', column: 'progress', priority: 'High', labels: ['Frontend', 'Design'], assignees: ['jk', 'ar'], comments: 4, files: 2, progress: 35 },
  { id: 6, title: 'Rate-limit the signup endpoint', column: 'review', priority: 'High', labels: ['Backend'], assignees: ['ml', 'jk'], due: 'Apr 30', comments: 3, files: 0 },
  { id: 7, title: 'Customer interview synthesis', column: 'done', priority: 'Low', labels: ['Research'], assignees: ['ar'], comments: 0, files: 4 },
];
