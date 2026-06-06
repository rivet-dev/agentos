#include <pthread.h>
#ifndef pthread_cleanup_pop
void (*foo)(int) = pthread_cleanup_pop;
#endif
int main(void) { return 0; }
