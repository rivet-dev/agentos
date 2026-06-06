#include <pthread.h>
#ifdef pthread_spin_init
#undef pthread_spin_init
#endif
int (*foo)(pthread_spinlock_t *, int) = pthread_spin_init;
int main(void) { return 0; }
