#include <pthread.h>
#ifdef pthread_spin_destroy
#undef pthread_spin_destroy
#endif
int (*foo)(pthread_spinlock_t *) = pthread_spin_destroy;
int main(void) { return 0; }
