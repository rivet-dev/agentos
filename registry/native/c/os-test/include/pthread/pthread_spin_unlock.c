#include <pthread.h>
#ifdef pthread_spin_unlock
#undef pthread_spin_unlock
#endif
int (*foo)(pthread_spinlock_t *) = pthread_spin_unlock;
int main(void) { return 0; }
