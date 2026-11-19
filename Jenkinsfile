pipeline {
    agent { label 'linux && dornguard' }

    options {
        timeout(time: 15, unit: 'MINUTES')
    }

    environment {
        DORNGUARD_ARCHUNIT_RUNNER = "${WORKSPACE}/runners/archunit-runner/build/libs/archunit-runner.jar"
        DORNGUARD_ENGINE_DIR = "${WORKSPACE}/.ci-tools/managed"
    }

    stages {
        stage('Install tools') {
            steps {
                sh 'scripts/ci-tools.sh'
            }
        }

        stage('Test') {
            steps {
                sh 'cargo test --locked --workspace -- --include-ignored'
            }
        }

        stage('Quality gate') {
            steps {
                sh 'cargo run --locked -- check examples/spring-boot-multimodule --format sarif > dornguard.sarif'
            }
        }
    }

    post {
        always {
            archiveArtifacts artifacts: 'dornguard.sarif', allowEmptyArchive: true
        }
    }
}
