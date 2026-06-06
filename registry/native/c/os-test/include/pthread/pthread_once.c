#include <pthread.h>
#ifdef pthread_once
#undef pthread_once
#endif
int (*foo)(pthread_once_t *, void (*)(void)) = pthread_once;
int main(void) { return 0; }
