#include <semaphore.h>
#ifdef sem_wait
#undef sem_wait
#endif
int (*foo)(sem_t *) = sem_wait;
int main(void) { return 0; }
