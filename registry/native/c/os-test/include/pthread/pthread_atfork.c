/*[OB]*/
#include <pthread.h>
#ifdef pthread_atfork
#undef pthread_atfork
#endif
int (*foo)(void (*)(void), void (*)(void), void(*)(void)) = pthread_atfork;
int main(void) { return 0; }
