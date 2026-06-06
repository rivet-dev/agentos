#include <pthread.h>
#ifdef pthread_spin_lock
#undef pthread_spin_lock
#endif
int (*foo)(pthread_spinlock_t *) = pthread_spin_lock;
int main(void) { return 0; }
