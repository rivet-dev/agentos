#include <pthread.h>
#ifdef pthread_setspecific
#undef pthread_setspecific
#endif
int (*foo)(pthread_key_t, const void *) = pthread_setspecific;
int main(void) { return 0; }
