#include <pthread.h>
#ifdef pthread_key_create
#undef pthread_key_create
#endif
int (*foo)(pthread_key_t *, void (*)(void*)) = pthread_key_create;
int main(void) { return 0; }
