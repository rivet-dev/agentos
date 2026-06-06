#include <semaphore.h>
#ifdef sem_trywait
#undef sem_trywait
#endif
int (*foo)(sem_t *) = sem_trywait;
int main(void) { return 0; }
