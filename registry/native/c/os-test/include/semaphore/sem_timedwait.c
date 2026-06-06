#include <semaphore.h>
#ifdef sem_timedwait
#undef sem_timedwait
#endif
int (*foo)(sem_t *restrict, const struct timespec *restrict) = sem_timedwait;
int main(void) { return 0; }
