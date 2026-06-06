#include <pthread.h>
#ifdef pthread_condattr_destroy
#undef pthread_condattr_destroy
#endif
int (*foo)(pthread_condattr_t *) = pthread_condattr_destroy;
int main(void) { return 0; }
