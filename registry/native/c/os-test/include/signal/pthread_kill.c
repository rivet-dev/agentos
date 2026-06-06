#include <signal.h>
#ifdef pthread_kill
#undef pthread_kill
#endif
int (*foo)(pthread_t, int) = pthread_kill;
int main(void) { return 0; }
