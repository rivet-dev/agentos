#include <pthread.h>
#ifdef pthread_condattr_init
#undef pthread_condattr_init
#endif
int (*foo)(pthread_condattr_t *) = pthread_condattr_init;
int main(void) { return 0; }
