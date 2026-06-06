#include <pthread.h>
#ifndef pthread_cleanup_push
void (*foo)(void (*)(void*), void *) = pthread_cleanup_push;
#endif
int main(void) { return 0; }
