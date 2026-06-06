#include <semaphore.h>
#ifdef sem_clockwait
#undef sem_clockwait
#endif
int (*foo)(sem_t *restrict, clockid_t, const struct timespec *restrict) = sem_clockwait;
int main(void) { return 0; }
