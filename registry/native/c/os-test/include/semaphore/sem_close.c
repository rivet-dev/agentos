#include <semaphore.h>
#ifdef sem_close
#undef sem_close
#endif
int (*foo)(sem_t *) = sem_close;
int main(void) { return 0; }
