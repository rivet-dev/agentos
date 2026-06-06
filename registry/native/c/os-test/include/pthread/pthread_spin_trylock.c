#include <pthread.h>
#ifdef pthread_spin_trylock
#undef pthread_spin_trylock
#endif
int (*foo)(pthread_spinlock_t *) = pthread_spin_trylock;
int main(void) { return 0; }
