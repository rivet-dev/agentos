#include <aio.h>
#ifdef lio_listio
#undef lio_listio
#endif
int (*foo)(int, struct aiocb *restrict const [restrict], int, struct sigevent *restrict) = lio_listio;
int main(void) { return 0; }
