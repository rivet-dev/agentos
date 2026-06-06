#include <pthread.h>
#ifdef pthread_testcancel
#undef pthread_testcancel
#endif
void (*foo)(void) = pthread_testcancel;
int main(void) { return 0; }
