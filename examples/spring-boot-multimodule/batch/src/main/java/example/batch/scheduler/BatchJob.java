package example.batch.scheduler;

import org.springframework.scheduling.annotation.Scheduled;

public final class BatchJob {
    @Scheduled(fixedDelay = 60000)
    public void run() {}
}
