/*[TPS]*/
#include <pthread.h>
#ifdef pthread_attr_setscope
#undef pthread_attr_setscope
#endif
int (*foo)(pthread_attr_t *, int) = pthread_attr_setscope;
int main(void) { return 0; }
